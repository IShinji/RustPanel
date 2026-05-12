#!/usr/bin/env node

type OwnerType = 'auto' | 'org' | 'user'

type ParsedArgs = {
  dryRun: boolean
  execute: boolean
  includeUntagged: boolean
  keepSha: number
  owner: string
  ownerType: OwnerType
  packageName: string
  packageType: string
  selfTest: boolean
}

type PackageVersion = {
  created_at: string
  id: number
  metadata?: {
    container?: {
      tags?: string[]
    }
  }
  name?: string
  updated_at: string
}

type CleanupPlan = {
  deleteVersions: PackageVersion[]
  keepVersions: PackageVersion[]
  protectedVersions: PackageVersion[]
}

type GitHubResponse<T> = {
  data: T | null
  status: number
  text: string
}

const GITHUB_API_VERSION = '2022-11-28'
const DEFAULT_KEEP_SHA = 15

const args = parseArgs(process.argv.slice(2))

if (args.selfTest) {
  runSelfTest()
  process.exit(0)
}

cleanupPackageVersions(args).catch((error: unknown) => {
  fail(errorMessage(error))
})

async function cleanupPackageVersions(parsed: ParsedArgs) {
  const token = process.env.GHCR_CLEANUP_TOKEN || process.env.GITHUB_TOKEN || process.env.GH_TOKEN
  if (!token) fail('GHCR_CLEANUP_TOKEN, GITHUB_TOKEN, or GH_TOKEN is required')

  const resolved = await listPackageVersionsWithOwnerType(parsed, token)
  const plan = createCleanupPlan(resolved.versions, parsed.keepSha, parsed.includeUntagged)

  printPlan({ ownerType: resolved.ownerType, parsed, plan })

  if (plan.deleteVersions.length === 0) {
    console.log('No GHCR package versions need cleanup.')
    return
  }
  if (!parsed.execute) {
    console.log('Dry run only. Pass --execute to delete the versions listed above.')
    return
  }

  for (const version of plan.deleteVersions) {
    await deletePackageVersion({
      ownerType: resolved.ownerType,
      parsed,
      token,
      versionId: version.id,
    })
    console.log(`Deleted package version ${version.id} (${tagsOf(version).join(', ') || 'untagged'})`)
  }
}

async function listPackageVersionsWithOwnerType(parsed: ParsedArgs, token: string): Promise<{
  ownerType: Exclude<OwnerType, 'auto'>
  versions: PackageVersion[]
}> {
  const ownerTypes: Exclude<OwnerType, 'auto'>[] =
    parsed.ownerType === 'auto' ? ['user', 'org'] : [parsed.ownerType]

  const failures: string[] = []
  for (const ownerType of ownerTypes) {
    const response = await listPackageVersions(parsed, ownerType, token)
    if (response.status === 200 && response.data) {
      return { ownerType, versions: response.data }
    }
    failures.push(`${ownerType}: ${response.status} ${response.text.slice(0, 200)}`)
  }

  fail(`Unable to list package versions for ${parsed.owner}/${parsed.packageName}. ${failures.join(' | ')}`)
}

async function listPackageVersions(
  parsed: ParsedArgs,
  ownerType: Exclude<OwnerType, 'auto'>,
  token: string,
): Promise<GitHubResponse<PackageVersion[]>> {
  const allVersions: PackageVersion[] = []
  const perPage = 100

  for (let page = 1; page <= 10; page += 1) {
    const query = new URLSearchParams({
      page: String(page),
      per_page: String(perPage),
      state: 'active',
    })
    const response = await githubRequest<PackageVersion[]>({
      expectedStatuses: [200, 404],
      pathName: packagePath(parsed, ownerType, `/versions?${query.toString()}`),
      token,
    })
    if (response.status !== 200 || !response.data) return response
    allVersions.push(...response.data)
    if (response.data.length < perPage) {
      return { data: allVersions, status: 200, text: JSON.stringify(allVersions) }
    }
  }

  return { data: allVersions, status: 200, text: JSON.stringify(allVersions) }
}

async function deletePackageVersion(input: {
  ownerType: Exclude<OwnerType, 'auto'>
  parsed: ParsedArgs
  token: string
  versionId: number
}) {
  await githubRequest<null>({
    expectedStatuses: [204],
    method: 'DELETE',
    pathName: packagePath(input.parsed, input.ownerType, `/versions/${input.versionId}`),
    token: input.token,
  })
}

async function githubRequest<T>(input: {
  expectedStatuses: number[]
  method?: string
  pathName: string
  token: string
}): Promise<GitHubResponse<T>> {
  const url = `https://api.github.com${input.pathName}`
  const response = await fetch(url, {
    headers: {
      Accept: 'application/vnd.github+json',
      Authorization: `Bearer ${input.token}`,
      'User-Agent': 'rustpanel-ghcr-cleanup',
      'X-GitHub-Api-Version': GITHUB_API_VERSION,
    },
    method: input.method ?? 'GET',
  })
  const text = await response.text()
  if (!input.expectedStatuses.includes(response.status)) {
    fail(`GitHub API ${input.method ?? 'GET'} ${url} failed with ${response.status}: ${text.slice(0, 800)}`)
  }
  if (!text) {
    return { data: null, status: response.status, text }
  }
  return { data: JSON.parse(text) as T, status: response.status, text }
}

function createCleanupPlan(versions: PackageVersion[], keepSha: number, includeUntagged: boolean): CleanupPlan {
  const shaCandidates = versions
    .filter((version) => isShaOnlyVersion(version))
    .sort((left, right) => compareVersionRecency(right, left))
  const keepShaIds = new Set(shaCandidates.slice(0, keepSha).map((version) => version.id))
  const keepVersions = shaCandidates.filter((version) => keepShaIds.has(version.id))
  const deleteVersions = shaCandidates.filter((version) => !keepShaIds.has(version.id))
  const protectedVersions = versions.filter((version) => !isShaOnlyVersion(version))

  if (includeUntagged) {
    const untagged = versions
      .filter((version) => tagsOf(version).length === 0)
      .sort((left, right) => compareVersionRecency(right, left))
    deleteVersions.push(...untagged)
  }

  return { deleteVersions, keepVersions, protectedVersions }
}

function printPlan(input: {
  ownerType: Exclude<OwnerType, 'auto'>
  parsed: ParsedArgs
  plan: CleanupPlan
}) {
  const mode = input.parsed.execute ? 'execute' : 'dry-run'
  console.log(
    `GHCR cleanup (${mode}): ${input.ownerType}/${input.parsed.owner}/${input.parsed.packageName}, ` +
      `keep ${input.parsed.keepSha} sha-tagged version(s).`,
  )
  console.log(`Protected versions: ${input.plan.protectedVersions.length}`)
  console.log(`Kept sha versions: ${input.plan.keepVersions.length}`)
  console.log(`Deletable versions: ${input.plan.deleteVersions.length}`)

  for (const version of input.plan.deleteVersions) {
    console.log(
      `- delete ${version.id}: ${tagsOf(version).join(', ') || 'untagged'} ` +
        `(updated ${version.updated_at || version.created_at})`,
    )
  }
}

function packagePath(parsed: ParsedArgs, ownerType: Exclude<OwnerType, 'auto'>, suffix: string): string {
  const owner = encodeURIComponent(parsed.owner)
  const packageType = encodeURIComponent(parsed.packageType)
  const packageName = encodeURIComponent(parsed.packageName)
  const ownerPrefix = ownerType === 'org' ? `/orgs/${owner}` : `/users/${owner}`
  return `${ownerPrefix}/packages/${packageType}/${packageName}${suffix}`
}

function isShaOnlyVersion(version: PackageVersion): boolean {
  const tags = tagsOf(version)
  return tags.length > 0 && tags.every(isShaTag)
}

function isShaTag(tag: string): boolean {
  return /^sha-[a-f0-9]{7,64}$/i.test(tag)
}

function tagsOf(version: PackageVersion): string[] {
  return version.metadata?.container?.tags ?? []
}

function compareVersionRecency(left: PackageVersion, right: PackageVersion): number {
  const leftMs = Date.parse(left.updated_at || left.created_at || '')
  const rightMs = Date.parse(right.updated_at || right.created_at || '')
  return leftMs - rightMs || left.id - right.id
}

function parseArgs(argv: string[]): ParsedArgs {
  const parsed: ParsedArgs = {
    dryRun: false,
    execute: false,
    includeUntagged: false,
    keepSha: DEFAULT_KEEP_SHA,
    owner: '',
    ownerType: 'auto',
    packageName: '',
    packageType: 'container',
    selfTest: false,
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--self-test') {
      parsed.selfTest = true
    } else if (arg === '--dry-run') {
      parsed.dryRun = true
    } else if (arg === '--execute') {
      parsed.execute = true
    } else if (arg === '--include-untagged') {
      parsed.includeUntagged = true
    } else if (arg === '--owner') {
      parsed.owner = readValue(argv, index, arg)
      index += 1
    } else if (arg === '--owner-type') {
      parsed.ownerType = parseOwnerType(readValue(argv, index, arg))
      index += 1
    } else if (arg === '--package') {
      parsed.packageName = readValue(argv, index, arg)
      index += 1
    } else if (arg === '--package-type') {
      parsed.packageType = readValue(argv, index, arg)
      index += 1
    } else if (arg === '--keep-sha') {
      parsed.keepSha = parsePositiveInteger(readValue(argv, index, arg), arg)
      index += 1
    } else {
      fail(`Unknown argument: ${arg}`)
    }
  }

  if (parsed.selfTest) return parsed
  if (!parsed.owner) fail('--owner is required')
  if (!parsed.packageName) fail('--package is required')
  if (parsed.dryRun && parsed.execute) fail('Use only one of --dry-run or --execute')
  if (!parsed.dryRun && !parsed.execute) parsed.dryRun = true
  validateSingleLine(parsed.owner, '--owner')
  validateSingleLine(parsed.packageName, '--package')
  validateSingleLine(parsed.packageType, '--package-type')

  return parsed
}

function parseOwnerType(value: string): OwnerType {
  if (value === 'auto' || value === 'org' || value === 'user') return value
  fail('--owner-type must be auto, org, or user')
}

function readValue(argv: string[], index: number, arg: string): string {
  const value = argv[index + 1]
  if (!value || value.startsWith('--')) fail(`${arg} requires a value`)
  return value
}

function parsePositiveInteger(value: string, arg: string): number {
  const parsed = Number.parseInt(value, 10)
  if (!Number.isInteger(parsed) || parsed < 0) {
    fail(`${arg} must be a non-negative integer`)
  }
  return parsed
}

function validateSingleLine(value: string, label: string) {
  if (value.includes('\n') || value.includes('\r')) {
    fail(`${label} must be a single-line value`)
  }
}

function runSelfTest() {
  const versions: PackageVersion[] = [
    version(1, ['latest', 'sha-1111111'], '2026-01-05T00:00:00Z'),
    version(2, ['sha-2222222'], '2026-01-04T00:00:00Z'),
    version(3, ['sha-3333333'], '2026-01-03T00:00:00Z'),
    version(4, ['sha-4444444'], '2026-01-02T00:00:00Z'),
    version(5, ['buildcache'], '2026-01-01T00:00:00Z'),
    version(6, [], '2025-12-31T00:00:00Z'),
  ]

  const plan = createCleanupPlan(versions, 2, false)
  assert(plan.keepVersions.map((item) => item.id).join(',') === '2,3', 'should keep the newest sha versions')
  assert(plan.deleteVersions.map((item) => item.id).join(',') === '4', 'should delete only old sha-only versions')
  assert(plan.protectedVersions.some((item) => item.id === 1), 'latest tag must be protected')
  assert(plan.protectedVersions.some((item) => item.id === 5), 'non-sha tags must be protected')
  assert(plan.protectedVersions.some((item) => item.id === 6), 'untagged versions must be protected by default')

  const untaggedPlan = createCleanupPlan(versions, 2, true)
  assert(untaggedPlan.deleteVersions.some((item) => item.id === 6), 'include-untagged should delete untagged versions')

  console.log('GHCR package cleanup self-test passed.')
}

function version(id: number, tags: string[], timestamp: string): PackageVersion {
  return {
    created_at: timestamp,
    id,
    metadata: { container: { tags } },
    updated_at: timestamp,
  }
}

function assert(condition: boolean, message: string) {
  if (!condition) {
    throw new Error(message)
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function fail(message: string): never {
  console.error(message)
  process.exit(1)
}
