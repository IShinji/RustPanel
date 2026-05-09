#!/usr/bin/env node

import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

type ParsedArgs = {
  cacheRoot: string
  family: string
  project: string
  cacheName: string
  localDest: string
  registryCaches: string[]
  maxPeers: number
  includeLegacyRootPeers: boolean
  selfTest: boolean
}

type CacheCandidate = {
  label: string
  path: string
  updatedAtMs: number
}

type CacheOutputs = {
  cacheFrom: string[]
  cacheTo: string[]
  summary: string[]
}

const args = parseArgs(process.argv.slice(2))

if (args.selfTest) {
  runSelfTest()
  process.exit(0)
}

const outputs = collectCacheOutputs(args)
writeOutputs(outputs)

function parseArgs(argv: string[]): ParsedArgs {
  const parsed: ParsedArgs = {
    cacheRoot: '',
    family: '',
    project: '',
    cacheName: '',
    localDest: '',
    registryCaches: [],
    maxPeers: 6,
    includeLegacyRootPeers: false,
    selfTest: false,
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--self-test') {
      parsed.selfTest = true
    } else if (arg === '--cache-root') {
      parsed.cacheRoot = readValue(argv, index, arg)
      index += 1
    } else if (arg === '--family') {
      parsed.family = normalizeSlug(readValue(argv, index, arg))
      index += 1
    } else if (arg === '--project') {
      parsed.project = normalizeSlug(readValue(argv, index, arg))
      index += 1
    } else if (arg === '--cache-name') {
      parsed.cacheName = normalizeSlug(readValue(argv, index, arg))
      index += 1
    } else if (arg === '--local-dest') {
      parsed.localDest = argv[index + 1] ?? ''
      index += 1
    } else if (arg === '--registry-cache') {
      parsed.registryCaches.push(readValue(argv, index, arg))
      index += 1
    } else if (arg === '--max-peers') {
      parsed.maxPeers = parsePositiveInteger(readValue(argv, index, arg), arg)
      index += 1
    } else if (arg === '--include-legacy-root-peers') {
      parsed.includeLegacyRootPeers = true
    } else {
      fail(`Unknown argument: ${arg}`)
    }
  }

  if (parsed.selfTest) return parsed
  if (!parsed.cacheRoot || !path.isAbsolute(parsed.cacheRoot)) {
    fail('--cache-root must be an absolute path')
  }
  if (!parsed.family) fail('--family is required')
  if (!parsed.project) fail('--project is required')
  if (!parsed.cacheName) fail('--cache-name is required')
  for (const value of [parsed.cacheRoot, parsed.family, parsed.project, parsed.cacheName, parsed.localDest, ...parsed.registryCaches]) {
    if (value.includes('\n')) {
      fail('arguments must be single-line values')
    }
  }

  return parsed
}

function collectCacheOutputs(parsed: ParsedArgs): CacheOutputs {
  const cacheFrom: string[] = []
  const cacheTo: string[] = []
  const summary: string[] = []
  const includedPaths = new Set<string>()
  const projectCachePath = path.join(parsed.cacheRoot, parsed.family, parsed.project, parsed.cacheName)

  addLocalImport({
    cacheFrom,
    includedPaths,
    label: `${parsed.project} local`,
    path: projectCachePath,
    summary,
  })

  if (parsed.includeLegacyRootPeers) {
    addLocalImport({
      cacheFrom,
      includedPaths,
      label: `${parsed.project} legacy local`,
      path: path.join(parsed.cacheRoot, parsed.project, parsed.cacheName),
      summary,
    })
  }

  const peerCandidates = [
    ...discoverPeerCaches(path.join(parsed.cacheRoot, parsed.family), parsed.project, parsed.cacheName, 'family peer'),
    ...(parsed.includeLegacyRootPeers
      ? discoverPeerCaches(parsed.cacheRoot, parsed.project, parsed.cacheName, 'legacy peer')
      : []),
  ]
    .filter((candidate) => !includedPaths.has(candidate.path))
    .sort((left, right) => right.updatedAtMs - left.updatedAtMs)

  let peerCount = 0
  for (const candidate of peerCandidates) {
    if (peerCount >= parsed.maxPeers) break
    const added = addLocalImport({
      cacheFrom,
      includedPaths,
      label: candidate.label,
      path: candidate.path,
      summary,
    })
    if (added) peerCount += 1
  }

  for (const registryCache of parsed.registryCaches.filter(Boolean)) {
    cacheFrom.push(`type=registry,ref=${registryCache}`)
    cacheTo.push(`type=registry,ref=${registryCache},mode=max,image-manifest=true,oci-mediatypes=true`)
    summary.push(`registry: ${registryCache}`)
  }

  if (parsed.localDest) {
    cacheTo.unshift(`type=local,dest=${parsed.localDest},mode=max`)
    summary.push(`local export: ${parsed.localDest}`)
  }

  if (cacheFrom.length === 0) {
    summary.push('no cache imports configured')
  }
  if (cacheTo.length === 0) {
    summary.push('no cache exports configured')
  }

  return { cacheFrom, cacheTo, summary }
}

function addLocalImport(input: {
  cacheFrom: string[]
  includedPaths: Set<string>
  label: string
  path: string
  summary: string[]
}): boolean {
  const normalizedPath = path.resolve(input.path)
  if (input.includedPaths.has(normalizedPath)) {
    return false
  }
  input.includedPaths.add(normalizedPath)

  if (!isCacheLayoutPresent(normalizedPath)) {
    console.log(`Skipping ${input.label} BuildKit cache import; cache not warmed yet: ${normalizedPath}`)
    return false
  }
  if (!validateCache(normalizedPath, input.label)) {
    console.log(`Skipping ${input.label} BuildKit cache import; cache is invalid: ${normalizedPath}`)
    return false
  }

  console.log(`Using ${input.label} BuildKit cache import: ${normalizedPath}`)
  input.cacheFrom.push(`type=local,src=${normalizedPath}`)
  input.summary.push(`${input.label}: ${normalizedPath}`)
  return true
}

function discoverPeerCaches(root: string, project: string, cacheName: string, labelPrefix: string): CacheCandidate[] {
  if (!fs.existsSync(root)) return []

  const candidates: CacheCandidate[] = []
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue

    const peerProject = normalizeSlug(entry.name)
    if (!peerProject || peerProject === project) continue

    const cachePath = path.join(root, entry.name, cacheName)
    if (!isCacheLayoutPresent(cachePath)) continue

    candidates.push({
      label: `${labelPrefix} ${peerProject}`,
      path: cachePath,
      updatedAtMs: cacheUpdatedAtMs(cachePath),
    })
  }

  return candidates
}

function isCacheLayoutPresent(cachePath: string): boolean {
  return (
    fs.existsSync(path.join(cachePath, 'index.json')) &&
    fs.existsSync(path.join(cachePath, 'blobs', 'sha256'))
  )
}

function cacheUpdatedAtMs(cachePath: string): number {
  try {
    return fs.statSync(path.join(cachePath, 'index.json')).mtimeMs
  } catch {
    return 0
  }
}

function validateCache(cachePath: string, label: string): boolean {
  const validatorPath = path.join(__dirname, 'validate-buildkit-local-cache.js')
  if (!fs.existsSync(validatorPath)) {
    return true
  }

  const result = spawnSync(process.execPath, [validatorPath, '--path', cachePath, '--label', label], {
    encoding: 'utf8',
  })
  if (result.stdout.trim()) console.log(result.stdout.trim())
  if (result.stderr.trim()) console.warn(result.stderr.trim())
  return result.status === 0
}

function writeOutputs(outputs: CacheOutputs) {
  console.log('BuildKit cache imports:')
  for (const line of outputs.cacheFrom) console.log(`- ${line}`)
  console.log('BuildKit cache exports:')
  for (const line of outputs.cacheTo) console.log(`- ${line}`)

  const githubOutput = process.env.GITHUB_OUTPUT
  if (!githubOutput) return

  appendGithubOutput(githubOutput, 'cache-from', outputs.cacheFrom.join('\n'))
  appendGithubOutput(githubOutput, 'cache-to', outputs.cacheTo.join('\n'))
  appendGithubOutput(githubOutput, 'summary', outputs.summary.join('\n'))
}

function appendGithubOutput(filePath: string, name: string, value: string) {
  const delimiter = `EOF_${name.replace(/[^A-Za-z0-9_]/g, '_')}_${process.pid}`
  fs.appendFileSync(filePath, `${name}<<${delimiter}\n${value}\n${delimiter}\n`)
}

function runSelfTest() {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'buildkit-cache-imports-test-'))
  try {
    const family = 'rust-backend-linux-amd64-v1'
    const project = 'app-main'
    const peerProjects = Array.from({ length: 4 }, (_, index) => `app-peer-${index + 1}`)
    const legacyProject = 'legacy-app'
    for (const cacheProject of [project, ...peerProjects]) {
      writeTestCache(path.join(tempRoot, family, cacheProject, 'backend-image'))
    }
    writeTestCache(path.join(tempRoot, legacyProject, 'backend-image'))

    const outputs = collectCacheOutputs({
      cacheRoot: tempRoot,
      family,
      project,
      cacheName: 'backend-image',
      localDest: path.join(tempRoot, family, project, 'backend-image-next-1'),
      registryCaches: ['ghcr.io/example/app-backend:buildcache'],
      maxPeers: 6,
      includeLegacyRootPeers: true,
      selfTest: false,
    })

    assert(outputs.cacheFrom.some((line) => line.includes(`/${project}/backend-image`)), 'missing own local import')
    for (const peerProject of peerProjects) {
      assert(outputs.cacheFrom.some((line) => line.includes(`/${peerProject}/backend-image`)), `missing peer import: ${peerProject}`)
    }
    assert(outputs.cacheFrom.some((line) => line.includes(`/${legacyProject}/backend-image`)), 'missing legacy peer import')
    assert(outputs.cacheFrom.some((line) => line.includes('type=registry')), 'missing registry import')
    assert(outputs.cacheTo.some((line) => line.includes('type=local')), 'missing local export')
    assert(outputs.cacheTo.some((line) => line.includes('type=registry')), 'missing registry export')
  } finally {
    fs.rmSync(tempRoot, { force: true, recursive: true })
  }

  console.log('BuildKit cache import collector self-test passed.')
}

function writeTestCache(cachePath: string) {
  fs.mkdirSync(cachePath, { recursive: true })
  const configDigest = writeBlob(cachePath, '{}')
  const layerDigest = writeBlob(cachePath, 'layer')
  const manifestDigest = writeBlob(
    cachePath,
    JSON.stringify({
      schemaVersion: 2,
      config: { digest: configDigest },
      layers: [{ digest: layerDigest }],
    }),
  )
  fs.writeFileSync(
    path.join(cachePath, 'index.json'),
    JSON.stringify({
      schemaVersion: 2,
      manifests: [{ digest: manifestDigest }],
    }),
  )
}

function writeBlob(cachePath: string, content: string): string {
  const buffer = Buffer.from(content)
  const digest = crypto.createHash('sha256').update(buffer).digest('hex')
  const blobPath = path.join(cachePath, 'blobs', 'sha256', digest)
  fs.mkdirSync(path.dirname(blobPath), { recursive: true })
  fs.writeFileSync(blobPath, buffer)
  return `sha256:${digest}`
}

function normalizeSlug(value: string): string {
  return value.trim().toLowerCase().replace(/[^a-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '')
}

function readValue(argv: string[], index: number, arg: string): string {
  const value = argv[index + 1]
  if (!value) fail(`${arg} requires a value`)
  return value
}

function parsePositiveInteger(value: string, arg: string): number {
  const parsed = Number.parseInt(value, 10)
  if (!Number.isInteger(parsed) || parsed < 0) {
    fail(`${arg} must be a non-negative integer`)
  }
  return parsed
}

function assert(condition: boolean, message: string) {
  if (!condition) {
    throw new Error(message)
  }
}

function fail(message: string): never {
  console.error(message)
  process.exit(1)
}
