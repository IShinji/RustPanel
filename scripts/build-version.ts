#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

type ParsedArgs = {
  app: string
}

type BuildVersion = {
  app: string
  buildTime: string
  buildVersion: string
  dirty: boolean
  gitCommit: string
  productVersion: string
}

const args = parseArgs(process.argv.slice(2))
const productVersion = process.env.VERSION || readProductVersion(args.app)
const gitCommit = process.env.GIT_COMMIT || git(['rev-parse', 'HEAD'], 'unknown')
const buildTime = process.env.BUILD_TIME || new Date().toISOString()
const shortCommit = gitCommit === 'unknown' ? '' : gitCommit.slice(0, 12)
const buildVersion = process.env.BUILD_VERSION || (shortCommit ? `${productVersion}+${shortCommit}` : productVersion)

const output: BuildVersion = {
  app: args.app,
  buildTime,
  buildVersion,
  dirty: isDirty(),
  gitCommit,
  productVersion,
}

console.log(JSON.stringify(output, null, 2))

function parseArgs(argv: string[]): ParsedArgs {
  const parsed: ParsedArgs = { app: 'backend' }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--app') {
      parsed.app = readValue(argv, index, arg)
      index += 1
    } else {
      fail(`Unknown argument: ${arg}`)
    }
  }

  return parsed
}

function readProductVersion(app: string): string {
  if (app === 'backend') return readCargoTomlVersion(path.resolve('src/backend/Cargo.toml'))
  if (app === 'web') return readPackageVersionWithBackendFallback(path.resolve('src/web/package.json'))
  if (app === 'admin') return readPackageVersionWithBackendFallback(path.resolve('src/admin/package.json'))
  if (app === 'root') return readPackageJsonVersion(path.resolve('package.json'))
  fail(`Unknown app: ${app}`)
}

function readPackageVersionWithBackendFallback(filePath: string): string {
  if (fs.existsSync(filePath)) return readPackageJsonVersion(filePath)
  return readCargoTomlVersion(path.resolve('src/backend/Cargo.toml'))
}

function readCargoTomlVersion(filePath: string): string {
  const content = fs.readFileSync(filePath, 'utf8')
  const packageBlock = packageSection(content)
  const match = /^version\s*=\s*"([^"]+)"\s*$/m.exec(packageBlock)
  if (!match) fail(`${path.relative(process.cwd(), filePath)} is missing [package] version`)
  return match[1]
}

function readPackageJsonVersion(filePath: string): string {
  const json = JSON.parse(fs.readFileSync(filePath, 'utf8')) as { version?: unknown }
  if (typeof json.version !== 'string' || !json.version.trim()) {
    fail(`${path.relative(process.cwd(), filePath)} is missing a version field`)
  }
  return json.version.trim()
}

function packageSection(content: string): string {
  const packageHeader = /^\[package\]\s*$/m.exec(content)
  if (!packageHeader) fail('Cargo.toml is missing [package] section')
  const sectionStart = packageHeader.index + packageHeader[0].length
  const rest = content.slice(sectionStart)
  const nextSection = rest.search(/^\[/m)
  return nextSection === -1 ? rest : rest.slice(0, nextSection)
}

function git(args: string[], fallback: string): string {
  const result = spawnSync('git', args, { encoding: 'utf8' })
  if (result.status !== 0) return fallback
  return result.stdout.trim() || fallback
}

function isDirty(): boolean {
  const result = spawnSync('git', ['diff', '--quiet'], { encoding: 'utf8' })
  if (result.status === 1) return true
  const staged = spawnSync('git', ['diff', '--cached', '--quiet'], { encoding: 'utf8' })
  return staged.status === 1
}

function readValue(argv: string[], index: number, arg: string): string {
  const value = argv[index + 1]
  if (!value || value.startsWith('--')) fail(`${arg} requires a value`)
  return value
}

function fail(message: string): never {
  console.error(message)
  process.exit(1)
}
