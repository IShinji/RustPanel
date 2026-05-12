#!/usr/bin/env node

import { spawnSync } from 'node:child_process'

type ParsedArgs = {
  commit: string
  intervalSeconds: number
  minRuns: number
  repository: string
  selfTest: boolean
  timeoutSeconds: number
  wait: boolean
  workflowNames: string[]
}

type WorkflowRun = {
  conclusion: string | null
  created_at: string
  event: string
  head_branch: string | null
  head_sha: string
  html_url: string
  id: number
  name: string
  status: string
  updated_at: string
}

type WorkflowRunsResponse = {
  workflow_runs?: WorkflowRun[]
}

type Evaluation =
  | { state: 'pending'; considered: WorkflowRun[]; message: string; pending: WorkflowRun[] }
  | { state: 'success'; considered: WorkflowRun[]; passed: WorkflowRun[] }
  | { state: 'failure'; considered: WorkflowRun[]; failed: WorkflowRun[] }

const GITHUB_API_VERSION = '2022-11-28'
const GITHUB_REQUEST_TIMEOUT_MS = 30_000
const DEFAULT_INTERVAL_SECONDS = 20
const DEFAULT_TIMEOUT_SECONDS = 60 * 60
const SUCCESS_CONCLUSIONS = new Set(['success', 'skipped', 'neutral'])

const args = parseArgs(process.argv.slice(2))

if (args.selfTest) {
  runSelfTest()
  process.exit(0)
}

checkLatestCi(args).catch((error: unknown) => {
  fail(errorMessage(error))
})

async function checkLatestCi(parsed: ParsedArgs) {
  const repository = parsed.repository || detectRepository()
  const commit = parsed.commit || detectCommit()
  validateRepository(repository)
  validateCommit(commit)

  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN
  const deadline = Date.now() + parsed.timeoutSeconds * 1000
  let pollCount = 0

  while (true) {
    pollCount += 1
    const allRuns = await listWorkflowRuns({ commit, repository, token })
    const currentRunId = Number.parseInt(process.env.GITHUB_RUN_ID || '', 10)
    const filtered = allRuns.filter((run) => Number.isNaN(currentRunId) || run.id !== currentRunId)
    const considered = selectLatestRuns(filtered, parsed.workflowNames)
    const evaluation = evaluateRuns(considered, parsed.minRuns, parsed.workflowNames)

    printEvaluation({ commit, evaluation, pollCount, repository })

    if (evaluation.state === 'success') return
    if (evaluation.state === 'failure') {
      fail(`CI failed for ${repository}@${commit}`)
    }
    if (!parsed.wait) {
      fail(`CI is not complete for ${repository}@${commit}: ${evaluation.message}`)
    }
    if (Date.now() >= deadline) {
      fail(`Timed out waiting for CI after ${parsed.timeoutSeconds}s: ${evaluation.message}`)
    }

    await sleep(parsed.intervalSeconds * 1000)
  }
}

async function listWorkflowRuns(input: {
  commit: string
  repository: string
  token: string | undefined
}): Promise<WorkflowRun[]> {
  const runs: WorkflowRun[] = []
  const perPage = 100

  for (let page = 1; page <= 5; page += 1) {
    const query = new URLSearchParams({
      head_sha: input.commit,
      page: String(page),
      per_page: String(perPage),
    })
    const response = await githubRequest<WorkflowRunsResponse>({
      pathName: `/repos/${input.repository}/actions/runs?${query.toString()}`,
      token: input.token,
    })
    const pageRuns = response.workflow_runs ?? []
    runs.push(...pageRuns)
    if (pageRuns.length < perPage) break
  }

  return runs
}

async function githubRequest<T>(input: {
  pathName: string
  token: string | undefined
}): Promise<T> {
  const url = `https://api.github.com${input.pathName}`
  const headers: Record<string, string> = {
    Accept: 'application/vnd.github+json',
    'User-Agent': 'rustpanel-ci-check',
    'X-GitHub-Api-Version': GITHUB_API_VERSION,
  }
  if (input.token) {
    headers.Authorization = `Bearer ${input.token}`
  }

  for (let attempt = 1; attempt <= 4; attempt += 1) {
    const timeout = abortAfter(GITHUB_REQUEST_TIMEOUT_MS)
    try {
      const response = await fetch(url, { headers, signal: timeout.signal })
      const text = await response.text()
      if (!response.ok) {
        if (isRetryableStatus(response.status) && attempt < 4) {
          await sleep(retryDelayMs(attempt))
          continue
        }
        const authHint = input.token ? '' : ' Set GITHUB_TOKEN or GH_TOKEN if this repository is private or rate limited.'
        fail(`GitHub API GET ${url} failed with ${response.status}: ${text.slice(0, 800)}${authHint}`)
      }
      return JSON.parse(text) as T
    } catch (error) {
      if (attempt < 4) {
        await sleep(retryDelayMs(attempt))
        continue
      }
      throw error
    } finally {
      timeout.clear()
    }
  }

  fail(`GitHub API GET ${url} failed after retries`)
}

function selectLatestRuns(runs: WorkflowRun[], workflowNames: string[]): WorkflowRun[] {
  const wanted = new Set(workflowNames)
  const latestByName = new Map<string, WorkflowRun>()
  const sorted = [...runs].sort((left, right) => compareRunRecency(right, left))

  for (const run of sorted) {
    if (wanted.size > 0 && !wanted.has(run.name)) continue
    if (!latestByName.has(run.name)) {
      latestByName.set(run.name, run)
    }
  }

  return [...latestByName.values()].sort(compareRunName)
}

function evaluateRuns(runs: WorkflowRun[], minRuns: number, workflowNames: string[]): Evaluation {
  if (workflowNames.length > 0) {
    const foundNames = new Set(runs.map((run) => run.name))
    const missing = workflowNames.filter((name) => !foundNames.has(name))
    if (missing.length > 0) {
      return {
        considered: runs,
        message: `waiting for workflow runs: ${missing.join(', ')}`,
        pending: runs.filter((run) => run.status !== 'completed'),
        state: 'pending',
      }
    }
  }

  if (runs.length < minRuns) {
    return {
      considered: runs,
      message: `waiting for at least ${minRuns} workflow run(s), found ${runs.length}`,
      pending: runs,
      state: 'pending',
    }
  }

  const pending = runs.filter((run) => run.status !== 'completed')
  if (pending.length > 0) {
    return {
      considered: runs,
      message: `${pending.length} workflow run(s) still pending`,
      pending,
      state: 'pending',
    }
  }

  const failed = runs.filter((run) => !SUCCESS_CONCLUSIONS.has(run.conclusion ?? ''))
  if (failed.length > 0) {
    return { considered: runs, failed, state: 'failure' }
  }

  return { considered: runs, passed: runs, state: 'success' }
}

function printEvaluation(input: {
  commit: string
  evaluation: Evaluation
  pollCount: number
  repository: string
}) {
  const { commit, evaluation, pollCount, repository } = input
  const shortSha = commit.slice(0, 12)
  const prefix = `[${pollCount}] ${repository}@${shortSha}`

  if (evaluation.state === 'success') {
    console.log(`${prefix}: CI passed (${evaluation.passed.length} workflow run(s)).`)
  } else if (evaluation.state === 'failure') {
    console.error(`${prefix}: CI failed.`)
  } else {
    console.log(`${prefix}: ${evaluation.message}.`)
  }

  for (const run of evaluation.considered) {
    const conclusion = run.conclusion ?? '-'
    console.log(`- ${run.name} #${run.id}: ${run.status}/${conclusion} ${run.html_url}`)
  }
}

function parseArgs(argv: string[]): ParsedArgs {
  const parsed: ParsedArgs = {
    commit: '',
    intervalSeconds: DEFAULT_INTERVAL_SECONDS,
    minRuns: 1,
    repository: '',
    selfTest: false,
    timeoutSeconds: DEFAULT_TIMEOUT_SECONDS,
    wait: false,
    workflowNames: [],
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--self-test') {
      parsed.selfTest = true
    } else if (arg === '--wait') {
      parsed.wait = true
    } else if (arg === '--commit') {
      parsed.commit = readValue(argv, index, arg)
      index += 1
    } else if (arg === '--repo') {
      parsed.repository = readValue(argv, index, arg)
      index += 1
    } else if (arg === '--workflow') {
      parsed.workflowNames.push(readValue(argv, index, arg))
      index += 1
    } else if (arg === '--timeout-seconds') {
      parsed.timeoutSeconds = parsePositiveInteger(readValue(argv, index, arg), arg)
      index += 1
    } else if (arg === '--interval-seconds') {
      parsed.intervalSeconds = parsePositiveInteger(readValue(argv, index, arg), arg)
      index += 1
    } else if (arg === '--min-runs') {
      parsed.minRuns = parsePositiveInteger(readValue(argv, index, arg), arg)
      index += 1
    } else {
      fail(`Unknown argument: ${arg}`)
    }
  }

  if (parsed.selfTest) return parsed
  if (parsed.intervalSeconds < 1) fail('--interval-seconds must be at least 1')
  if (parsed.timeoutSeconds < parsed.intervalSeconds) {
    fail('--timeout-seconds must be greater than or equal to --interval-seconds')
  }

  return parsed
}

function detectRepository(): string {
  if (process.env.GITHUB_REPOSITORY) return process.env.GITHUB_REPOSITORY

  const remote = git(['config', '--get', 'remote.origin.url'])
  const httpsMatch = /^https:\/\/github\.com\/([^/]+)\/(.+?)(?:\.git)?$/.exec(remote)
  if (httpsMatch) return `${httpsMatch[1]}/${httpsMatch[2]}`

  const sshMatch = /^git@github\.com:([^/]+)\/(.+?)(?:\.git)?$/.exec(remote)
  if (sshMatch) return `${sshMatch[1]}/${sshMatch[2]}`

  fail('Unable to detect GitHub repository. Pass --repo owner/name.')
}

function detectCommit(): string {
  return process.env.GITHUB_SHA || git(['rev-parse', 'HEAD'])
}

function git(args: string[]): string {
  const result = spawnSync('git', args, { encoding: 'utf8' })
  if (result.status !== 0) {
    const detail = result.stderr.trim() || result.stdout.trim()
    fail(`git ${args.join(' ')} failed: ${detail}`)
  }
  return result.stdout.trim()
}

function validateRepository(repository: string) {
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
    fail(`Invalid repository: ${repository}`)
  }
}

function validateCommit(commit: string) {
  if (!/^[a-f0-9]{7,64}$/i.test(commit)) {
    fail(`Invalid commit SHA: ${commit}`)
  }
}

function compareRunName(left: WorkflowRun, right: WorkflowRun): number {
  return left.name.localeCompare(right.name) || left.id - right.id
}

function compareRunRecency(left: WorkflowRun, right: WorkflowRun): number {
  const leftMs = Date.parse(left.created_at || left.updated_at || '')
  const rightMs = Date.parse(right.created_at || right.updated_at || '')
  return leftMs - rightMs || left.id - right.id
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

function runSelfTest() {
  const baseRun: WorkflowRun = {
    conclusion: null,
    created_at: '2026-01-01T00:00:00Z',
    event: 'push',
    head_branch: 'main',
    head_sha: 'abc1234',
    html_url: 'https://github.com/example/repo/actions/runs/1',
    id: 1,
    name: 'Release CI & Deploy',
    status: 'queued',
    updated_at: '2026-01-01T00:00:00Z',
  }

  assert(evaluateRuns([], 1, []).state === 'pending', 'empty runs should be pending')
  assert(evaluateRuns([baseRun], 1, []).state === 'pending', 'queued run should be pending')
  assert(
    evaluateRuns([{ ...baseRun, conclusion: 'success', status: 'completed' }], 1, []).state === 'success',
    'successful run should pass',
  )
  assert(
    evaluateRuns([{ ...baseRun, conclusion: 'failure', status: 'completed' }], 1, []).state === 'failure',
    'failed run should fail',
  )

  const selected = selectLatestRuns([
    { ...baseRun, conclusion: 'failure', created_at: '2026-01-01T00:00:00Z', id: 1, status: 'completed' },
    { ...baseRun, conclusion: 'success', created_at: '2026-01-01T00:05:00Z', id: 2, status: 'completed' },
  ], [])
  assert(selected.length === 1 && selected[0].id === 2, 'latest run per workflow should be selected')

  console.log('CI async check self-test passed.')
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
