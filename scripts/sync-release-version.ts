#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'

type ParsedArgs = {
  bump: BumpKind | ''
  check: boolean
  selfTest: boolean
  version: string
  write: boolean
}

type BumpKind = 'major' | 'minor' | 'patch'

type VersionTarget = {
  kind: 'cargo-lock' | 'cargo-toml' | 'package-json'
  label: string
  path: string
}

type VersionIssue = {
  actual: string
  expected: string
  label: string
}

const BACKEND_CRATE = 'rustpanel-backend'
const BACKEND_CARGO_TOML = path.resolve('src/backend/Cargo.toml')
const CARGO_LOCK = path.resolve('Cargo.lock')
const PACKAGE_JSON_TARGETS = [
  path.resolve('package.json'),
  path.resolve('src/web/package.json'),
  path.resolve('src/admin/package.json'),
]

const args = parseArgs(process.argv.slice(2))

if (args.selfTest) {
  runSelfTest()
  process.exit(0)
}

syncReleaseVersion(args)

function syncReleaseVersion(parsed: ParsedArgs) {
  const currentVersion = readCargoTomlVersion(BACKEND_CARGO_TOML)
  const desiredVersion = desiredReleaseVersion(parsed, currentVersion)
  const targets = discoverTargets()
  const issues = collectIssues(targets, desiredVersion)

  if (issues.length === 0) {
    console.log(`release version check passed: ${desiredVersion}`)
    return
  }

  if (parsed.check) {
    for (const issue of issues) {
      console.error(`${issue.label}: expected ${issue.expected}, got ${issue.actual || '<missing>'}`)
    }
    fail('release versions are not synchronized')
  }

  for (const target of targets) {
    writeTargetVersion(target, desiredVersion)
  }
  console.log(`release versions synchronized: ${desiredVersion}`)
}

function desiredReleaseVersion(parsed: ParsedArgs, currentVersion: string): string {
  if (parsed.version && parsed.bump) fail('Use only one of --version or --bump')
  if (parsed.version) return validateSemver(parsed.version, '--version')
  if (parsed.bump) return bumpVersion(currentVersion, parsed.bump)
  return currentVersion
}

function discoverTargets(): VersionTarget[] {
  const targets: VersionTarget[] = []
  if (fs.existsSync(BACKEND_CARGO_TOML)) {
    targets.push({ kind: 'cargo-toml', label: 'src/backend/Cargo.toml', path: BACKEND_CARGO_TOML })
  }
  if (fs.existsSync(CARGO_LOCK)) {
    targets.push({ kind: 'cargo-lock', label: 'Cargo.lock rustpanel-backend', path: CARGO_LOCK })
  }
  for (const packagePath of PACKAGE_JSON_TARGETS) {
    if (!fs.existsSync(packagePath)) continue
    const relative = path.relative(process.cwd(), packagePath)
    targets.push({ kind: 'package-json', label: relative, path: packagePath })
  }
  return targets
}

function collectIssues(targets: VersionTarget[], expected: string): VersionIssue[] {
  const issues: VersionIssue[] = []
  for (const target of targets) {
    const actual = readTargetVersion(target)
    if (actual !== expected) {
      issues.push({ actual, expected, label: target.label })
    }
  }
  return issues
}

function readTargetVersion(target: VersionTarget): string {
  if (target.kind === 'cargo-toml') return readCargoTomlVersion(target.path)
  if (target.kind === 'cargo-lock') return readCargoLockVersion(target.path, BACKEND_CRATE)
  return readPackageJsonVersion(target.path)
}

function writeTargetVersion(target: VersionTarget, version: string) {
  if (target.kind === 'cargo-toml') {
    writeCargoTomlVersion(target.path, version)
  } else if (target.kind === 'cargo-lock') {
    writeCargoLockVersion(target.path, BACKEND_CRATE, version)
  } else {
    writePackageJsonVersion(target.path, version)
  }
}

function readCargoTomlVersion(filePath: string): string {
  const content = fs.readFileSync(filePath, 'utf8')
  const packageBlock = packageSection(content)
  const match = /^version\s*=\s*"([^"]+)"\s*$/m.exec(packageBlock)
  if (!match) fail(`${path.relative(process.cwd(), filePath)} is missing [package] version`)
  return validateSemver(match[1], `${path.relative(process.cwd(), filePath)} version`)
}

function writeCargoTomlVersion(filePath: string, version: string) {
  const content = fs.readFileSync(filePath, 'utf8')
  const updated = replaceInPackageSection(content, /^version\s*=\s*"[^"]+"\s*$/m, `version = "${version}"`)
  fs.writeFileSync(filePath, updated)
}

function readCargoLockVersion(filePath: string, crateName: string): string {
  const content = fs.readFileSync(filePath, 'utf8')
  const block = cargoLockPackageBlock(content, crateName)
  const match = /^version\s*=\s*"([^"]+)"\s*$/m.exec(block)
  if (!match) fail(`Cargo.lock is missing ${crateName} version`)
  return validateSemver(match[1], `Cargo.lock ${crateName} version`)
}

function writeCargoLockVersion(filePath: string, crateName: string, version: string) {
  const content = fs.readFileSync(filePath, 'utf8')
  const block = cargoLockPackageBlock(content, crateName)
  const updatedBlock = block.replace(/^version\s*=\s*"[^"]+"\s*$/m, `version = "${version}"`)
  fs.writeFileSync(filePath, content.replace(block, updatedBlock))
}

function readPackageJsonVersion(filePath: string): string {
  const json = readPackageJson(filePath)
  if (typeof json.version !== 'string') {
    fail(`${path.relative(process.cwd(), filePath)} is missing a version field`)
  }
  return validateSemver(json.version, `${path.relative(process.cwd(), filePath)} version`)
}

function writePackageJsonVersion(filePath: string, version: string) {
  const json = readPackageJson(filePath)
  json.version = version
  fs.writeFileSync(filePath, `${JSON.stringify(json, null, 2)}\n`)
}

function readPackageJson(filePath: string): Record<string, unknown> {
  return JSON.parse(fs.readFileSync(filePath, 'utf8')) as Record<string, unknown>
}

function packageSection(content: string): string {
  const packageHeader = /^\[package\]\s*$/m.exec(content)
  if (!packageHeader) fail('Cargo.toml is missing [package] section')
  const sectionStart = packageHeader.index + packageHeader[0].length
  const rest = content.slice(sectionStart)
  const nextSection = rest.search(/^\[/m)
  return nextSection === -1 ? rest : rest.slice(0, nextSection)
}

function replaceInPackageSection(content: string, pattern: RegExp, replacement: string): string {
  const packageHeader = /^\[package\]\s*$/m.exec(content)
  if (!packageHeader) fail('Cargo.toml is missing [package] section')
  const sectionStart = packageHeader.index + packageHeader[0].length
  const rest = content.slice(sectionStart)
  const nextSection = rest.search(/^\[/m)
  const sectionEnd = nextSection === -1 ? content.length : sectionStart + nextSection
  const block = content.slice(sectionStart, sectionEnd)
  if (!pattern.test(block)) fail('Cargo.toml [package] section is missing version')
  const updatedBlock = block.replace(pattern, replacement)
  return content.slice(0, sectionStart) + updatedBlock + content.slice(sectionEnd)
}

function cargoLockPackageBlock(content: string, crateName: string): string {
  const starts = [...content.matchAll(/^\[\[package\]\]\s*$/gm)].map((match) => match.index ?? 0)
  for (let index = 0; index < starts.length; index += 1) {
    const start = starts[index]
    const end = starts[index + 1] ?? content.length
    const block = content.slice(start, end)
    if (new RegExp(`^name\\s*=\\s*"${escapeRegExp(crateName)}"\\s*$`, 'm').test(block)) {
      return block
    }
  }
  fail(`Cargo.lock is missing package ${crateName}`)
}

function bumpVersion(version: string, kind: BumpKind): string {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(validateSemver(version, 'current version'))
  if (!match) fail(`Cannot bump non-simple semver version: ${version}`)
  const major = Number.parseInt(match[1], 10)
  const minor = Number.parseInt(match[2], 10)
  const patch = Number.parseInt(match[3], 10)
  if (kind === 'major') return `${major + 1}.0.0`
  if (kind === 'minor') return `${major}.${minor + 1}.0`
  return `${major}.${minor}.${patch + 1}`
}

function validateSemver(version: string, label: string): string {
  const trimmed = version.trim()
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(trimmed)) {
    fail(`${label} must be a semver value, got: ${version}`)
  }
  return trimmed
}

function parseArgs(argv: string[]): ParsedArgs {
  const parsed: ParsedArgs = {
    bump: '',
    check: false,
    selfTest: false,
    version: '',
    write: false,
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--self-test') {
      parsed.selfTest = true
    } else if (arg === '--check') {
      parsed.check = true
    } else if (arg === '--write') {
      parsed.write = true
    } else if (arg === '--version') {
      parsed.version = readValue(argv, index, arg)
      index += 1
    } else if (arg === '--bump') {
      parsed.bump = parseBump(readValue(argv, index, arg))
      index += 1
    } else {
      fail(`Unknown argument: ${arg}`)
    }
  }

  if (!parsed.check) parsed.write = true
  if (parsed.check && parsed.write) fail('Use only one of --check or --write')
  return parsed
}

function parseBump(value: string): BumpKind {
  if (value === 'major' || value === 'minor' || value === 'patch') return value
  fail('--bump must be major, minor, or patch')
}

function readValue(argv: string[], index: number, arg: string): string {
  const value = argv[index + 1]
  if (!value || value.startsWith('--')) fail(`${arg} requires a value`)
  return value
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function runSelfTest() {
  assert(bumpVersion('1.2.3', 'patch') === '1.2.4', 'patch bump failed')
  assert(bumpVersion('1.2.3', 'minor') === '1.3.0', 'minor bump failed')
  assert(bumpVersion('1.2.3', 'major') === '2.0.0', 'major bump failed')
  console.log('release version sync self-test passed.')
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
