#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

type Args = {
  assetPath: string
  assetName: string
  contentType: string
  selfTest: boolean
  tag: string
  title: string
}

type GitHubAsset = {
  id: number
  name: string
  browser_download_url?: string
}

type GitHubRelease = {
  id: number
  tag_name: string
  name?: string
  upload_url?: string
  html_url?: string
  assets?: GitHubAsset[]
}

type GitHubResponse<T> = {
  data: T | null
  status: number
  text: string
}

const DEFAULT_ASSET_NAME = 'rustpanel-backend-linux-amd64.tar.gz'
const DEFAULT_TAG = 'micro-latest'
const DEFAULT_TITLE = 'RustPanel micro latest'
const GITHUB_API_VERSION = '2022-11-28'
const GITHUB_REQUEST_ATTEMPTS = 4
const GITHUB_REQUEST_TIMEOUT_MS = 30_000

const args = parseArgs(process.argv.slice(2))

if (args.selfTest) {
  runSelfTest()
  process.exit(0)
}

publish(args).catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error)
  console.error(message)
  process.exit(1)
})

async function publish(parsed: Args) {
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN
  const repository = process.env.GITHUB_REPOSITORY
  const sha = process.env.GITHUB_SHA

  if (!token) fail('GITHUB_TOKEN or GH_TOKEN is required')
  if (!repository) fail('GITHUB_REPOSITORY is required')
  if (!sha) fail('GITHUB_SHA is required')
  if (!fs.existsSync(parsed.assetPath)) fail(`Asset not found: ${parsed.assetPath}`)

  const assetStat = fs.statSync(parsed.assetPath)
  if (!assetStat.isFile()) fail(`Asset path is not a file: ${parsed.assetPath}`)
  if (assetStat.size <= 0) fail(`Asset is empty: ${parsed.assetPath}`)

  const release = await ensureRelease({ parsed, repository, sha, token })
  await deleteExistingAsset({ assetName: parsed.assetName, release, repository, token })
  const uploaded = await uploadAsset({ parsed, release, repository, token })
  console.log(`Published ${uploaded.name} to ${release.html_url ?? `${repository}@${parsed.tag}`}`)
}

async function ensureRelease(input: {
  parsed: Args
  repository: string
  sha: string
  token: string
}): Promise<GitHubRelease> {
  const { parsed, repository, sha, token } = input
  const releaseBody = releaseNotes(sha)
  const tagPath = `/repos/${repository}/releases/tags/${encodeURIComponent(parsed.tag)}`
  const existing = await githubRequest<GitHubRelease>({
    pathName: tagPath,
    token,
    expectedStatuses: [200, 404],
  })

  if (existing.status === 200 && existing.data) {
    const updated = await githubRequest<GitHubRelease>({
      pathName: `/repos/${repository}/releases/${existing.data.id}`,
      token,
      method: 'PATCH',
      body: {
        body: releaseBody,
        name: parsed.title,
        prerelease: true,
      },
      expectedStatuses: [200],
    })
    if (!updated.data) fail('GitHub returned an empty release update response')
    updated.data.assets = updated.data.assets ?? existing.data.assets
    return updated.data
  }

  const created = await githubRequest<GitHubRelease>({
    pathName: `/repos/${repository}/releases`,
    token,
    method: 'POST',
    body: {
      body: releaseBody,
      name: parsed.title,
      prerelease: true,
      tag_name: parsed.tag,
      target_commitish: sha,
    },
    expectedStatuses: [201],
  })
  if (!created.data) fail('GitHub returned an empty release create response')
  return created.data
}

async function deleteExistingAsset(input: {
  assetName: string
  release: GitHubRelease
  repository: string
  token: string
}) {
  const { assetName, release, repository, token } = input
  const assets = release.assets ?? []
  for (const asset of assets) {
    if (asset.name !== assetName) continue
    await githubRequest<null>({
      pathName: `/repos/${repository}/releases/assets/${asset.id}`,
      token,
      method: 'DELETE',
      expectedStatuses: [204],
    })
  }
}

async function uploadAsset(input: {
  parsed: Args
  release: GitHubRelease
  repository: string
  token: string
}): Promise<GitHubAsset> {
  const { parsed, release, repository, token } = input
  const uploadUrl = `https://uploads.github.com/repos/${repository}/releases/${release.id}/assets?name=${encodeURIComponent(parsed.assetName)}`
  return uploadAssetWithCurl({
    assetName: parsed.assetName,
    assetPath: parsed.assetPath,
    contentType: parsed.contentType,
    token,
    uploadUrl,
  })
}

function uploadAssetWithCurl(input: {
  assetName: string
  assetPath: string
  contentType: string
  token: string
  uploadUrl: string
}): GitHubAsset {
  const result = spawnSync('curl', [
    '--fail-with-body',
    '--silent',
    '--show-error',
    '--location',
    '--retry',
    '3',
    '--retry-delay',
    '2',
    '--retry-all-errors',
    '--connect-timeout',
    '30',
    '--max-time',
    '600',
    '--request',
    'POST',
    '--header',
    'Accept: application/vnd.github+json',
    '--header',
    `Authorization: Bearer ${input.token}`,
    '--header',
    `Content-Type: ${input.contentType}`,
    '--header',
    `X-GitHub-Api-Version: ${GITHUB_API_VERSION}`,
    '--data-binary',
    `@${input.assetPath}`,
    input.uploadUrl,
  ], {
    encoding: 'utf8',
    maxBuffer: 10 * 1024 * 1024,
  })

  if (result.status !== 0) {
    const detail = [result.stdout.trim(), result.stderr.trim()].filter(Boolean).join('\n')
    fail(`GitHub asset upload failed for ${input.assetName} with curl exit ${result.status}: ${detail.slice(0, 1200)}`)
  }

  return JSON.parse(result.stdout) as GitHubAsset
}

async function githubRequest<T>(input: {
  body?: Record<string, unknown>
  bodyBytes?: BodyInit
  contentLength?: number
  contentType?: string
  expectedStatuses: number[]
  method?: string
  pathName?: string
  token: string
  url?: string
}): Promise<GitHubResponse<T>> {
  const url = input.url ?? `https://api.github.com${input.pathName}`
  const headers: Record<string, string> = {
    Accept: 'application/vnd.github+json',
    Authorization: `Bearer ${input.token}`,
    'User-Agent': 'rustpanel-micro-release-publisher',
    'X-GitHub-Api-Version': GITHUB_API_VERSION,
  }

  let body: string | BodyInit | undefined
  if (input.body) {
    headers['Content-Type'] = 'application/json'
    body = JSON.stringify(input.body)
  } else if (input.bodyBytes) {
    headers['Content-Type'] = input.contentType ?? 'application/octet-stream'
    headers['Content-Length'] = String(input.contentLength ?? 0)
    body = input.bodyBytes
  }

  for (let attempt = 1; attempt <= GITHUB_REQUEST_ATTEMPTS; attempt += 1) {
    const timeout = abortAfter(GITHUB_REQUEST_TIMEOUT_MS)
    try {
      const response = await fetch(url, {
        body,
        headers,
        method: input.method ?? 'GET',
        signal: timeout.signal,
      })
      const text = await response.text()
      if (!input.expectedStatuses.includes(response.status)) {
        if (isRetryableStatus(response.status) && attempt < GITHUB_REQUEST_ATTEMPTS) {
          await sleep(retryDelayMs(attempt))
          continue
        }
        fail(`GitHub API ${input.method ?? 'GET'} ${url} failed with ${response.status}: ${text.slice(0, 800)}`)
      }

      if (!text) {
        return { data: null, status: response.status, text }
      }

      return { data: JSON.parse(text) as T, status: response.status, text }
    } catch (error) {
      if (attempt < GITHUB_REQUEST_ATTEMPTS) {
        await sleep(retryDelayMs(attempt))
        continue
      }
      fail(`GitHub API ${input.method ?? 'GET'} ${url} failed after retries: ${errorMessage(error)}`)
    } finally {
      timeout.clear()
    }
  }

  fail(`GitHub API ${input.method ?? 'GET'} ${url} failed after retries`)
}

function parseArgs(argv: string[]): Args {
  const parsed: Args = {
    assetPath: '',
    assetName: DEFAULT_ASSET_NAME,
    contentType: 'application/gzip',
    selfTest: false,
    tag: DEFAULT_TAG,
    title: DEFAULT_TITLE,
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--self-test') {
      parsed.selfTest = true
    } else if (arg === '--asset') {
      parsed.assetPath = path.resolve(readValue(argv, index, arg))
      index += 1
    } else if (arg === '--asset-name') {
      parsed.assetName = readValue(argv, index, arg)
      index += 1
    } else if (arg === '--content-type') {
      parsed.contentType = readValue(argv, index, arg)
      index += 1
    } else if (arg === '--tag') {
      parsed.tag = readValue(argv, index, arg)
      index += 1
    } else if (arg === '--title') {
      parsed.title = readValue(argv, index, arg)
      index += 1
    } else {
      fail(`Unknown argument: ${arg}`)
    }
  }

  if (!parsed.selfTest) {
    if (!parsed.assetPath) fail('--asset is required')
    validateSingleLine(parsed.assetName, '--asset-name')
    validateSingleLine(parsed.contentType, '--content-type')
    validateSingleLine(parsed.tag, '--tag')
    validateSingleLine(parsed.title, '--title')
  }

  return parsed
}

function readValue(argv: string[], index: number, flag: string): string {
  const value = argv[index + 1]
  if (!value || value.startsWith('--')) fail(`${flag} requires a value`)
  return value
}

function validateSingleLine(value: string, flag: string) {
  if (!value || value.includes('\n') || value.includes('\r')) {
    fail(`${flag} must be a non-empty single-line value`)
  }
}

function releaseNotes(sha: string): string {
  return `Automated linux/amd64 binary for RustPanel micro installs.\n\nCommit: ${sha}`
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function isRetryableStatus(status: number): boolean {
  return status === 429 || status >= 500
}

function retryDelayMs(attempt: number): number {
  return Math.min(10_000, 1000 * 2 ** (attempt - 1))
}

function abortAfter(ms: number): { clear: () => void; signal: AbortSignal } {
  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), ms)
  return {
    clear: () => clearTimeout(timeout),
    signal: controller.signal,
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function runSelfTest() {
  const parsed = parseArgs([
    '--asset',
    'dist/release/example.tar.gz',
    '--asset-name',
    DEFAULT_ASSET_NAME,
    '--tag',
    DEFAULT_TAG,
  ])
  if (!parsed.assetPath.endsWith('dist/release/example.tar.gz')) fail('self-test failed to parse asset path')
  if (!releaseNotes('abc123').includes('abc123')) fail('self-test failed to include commit in notes')
  console.log('micro release publisher self-test passed.')
}

function fail(message: string): never {
  throw new Error(message)
}
