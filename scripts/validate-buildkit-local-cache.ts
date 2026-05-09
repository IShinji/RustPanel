#!/usr/bin/env node

import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

type ParsedArgs = {
  label: string
  path: string
  quarantine: boolean
  selfTest: boolean
}

type CacheValidationResult =
  | { ok: true }
  | { ok: false; reason: string }

type DescriptorOptions = {
  parseNested: boolean
  visited: Set<string>
}

const args = parseArgs(process.argv.slice(2))

if (args.selfTest) {
  runSelfTest()
  process.exit(0)
}

if (!args.path) {
  fail('Usage: dist/node-scripts/scripts/validate-buildkit-local-cache.js --path <cache-dir> [--quarantine] [--label <name>]')
}

if (!fs.existsSync(args.path)) {
  console.log(`${args.label || 'BuildKit local cache'} is missing; the next build will warm it: ${args.path}`)
  process.exit(0)
}

const result = validateLocalCache(args.path)
if (result.ok) {
  console.log(`${args.label || 'BuildKit local cache'} is valid: ${args.path}`)
  process.exit(0)
}

if (args.quarantine) {
  const quarantinePath = quarantineCache(args.path)
  console.warn(
    `${args.label || 'BuildKit local cache'} is invalid and was quarantined: ${quarantinePath}\n` +
      `Reason: ${result.reason}`,
  )
  process.exit(0)
}

fail(`${args.label || 'BuildKit local cache'} is invalid: ${result.reason}`)

function parseArgs(argv: string[]): ParsedArgs {
  const parsed = {
    label: 'BuildKit local cache',
    path: '',
    quarantine: false,
    selfTest: false,
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--self-test') {
      parsed.selfTest = true
    } else if (arg === '--quarantine') {
      parsed.quarantine = true
    } else if (arg === '--path') {
      parsed.path = argv[index + 1] || ''
      index += 1
    } else if (arg === '--label') {
      parsed.label = argv[index + 1] || parsed.label
      index += 1
    } else {
      fail(`Unknown argument: ${arg}`)
    }
  }

  return parsed
}

function validateLocalCache(cachePath: string): CacheValidationResult {
  if (!fs.existsSync(cachePath)) {
    return { ok: true }
  }

  const indexPath = path.join(cachePath, 'index.json')
  const blobsPath = path.join(cachePath, 'blobs', 'sha256')
  if (!fs.existsSync(indexPath)) {
    return { ok: false, reason: `missing index.json in ${cachePath}` }
  }
  if (!fs.existsSync(blobsPath)) {
    return { ok: false, reason: `missing blobs/sha256 in ${cachePath}` }
  }

  let index: any
  try {
    index = readJson(indexPath)
  } catch (error) {
    return { ok: false, reason: `invalid index.json: ${errorMessage(error)}` }
  }

  if (!Array.isArray(index.manifests)) {
    return { ok: false, reason: 'index.json must contain a manifests array' }
  }

  const visited = new Set<string>()
  for (const descriptor of index.manifests) {
    const issue = validateDescriptor(cachePath, descriptor, { parseNested: true, visited })
    if (issue) {
      return { ok: false, reason: issue }
    }
  }

  return { ok: true }
}

function validateDescriptor(cachePath: string, descriptor: any, options: DescriptorOptions): string | null {
  if (!descriptor || typeof descriptor.digest !== 'string') {
    return 'descriptor is missing digest'
  }

  const blobPath = blobPathForDigest(cachePath, descriptor.digest)
  if (!blobPath) {
    return `unsupported digest: ${descriptor.digest}`
  }
  if (!fs.existsSync(blobPath)) {
    return `missing blob for ${descriptor.digest}`
  }

  if (!options.parseNested || options.visited.has(descriptor.digest)) {
    return null
  }
  options.visited.add(descriptor.digest)

  let object
  try {
    object = readJson(blobPath)
  } catch {
    return null
  }

  if (Array.isArray(object.manifests)) {
    for (const child of object.manifests) {
      const issue = validateDescriptor(cachePath, child, { parseNested: true, visited: options.visited })
      if (issue) {
        return issue
      }
    }
  }

  if (object.config) {
    const issue = validateDescriptor(cachePath, object.config, { parseNested: false, visited: options.visited })
    if (issue) {
      return issue
    }
  }

  if (Array.isArray(object.layers)) {
    for (const layer of object.layers) {
      const issue = validateDescriptor(cachePath, layer, { parseNested: false, visited: options.visited })
      if (issue) {
        return issue
      }
    }
  }

  return null
}

function blobPathForDigest(cachePath: string, digest: string): string | null {
  const match = /^sha256:([a-f0-9]{64})$/i.exec(digest)
  if (!match) {
    return null
  }
  return path.join(cachePath, 'blobs', 'sha256', match[1].toLowerCase())
}

function quarantineCache(cachePath: string): string {
  const timestamp = new Date().toISOString().replace(/[-:]/g, '').replace(/\..*$/, 'Z')
  let quarantinePath = `${cachePath}.invalid-${timestamp}`
  if (fs.existsSync(quarantinePath)) {
    quarantinePath = `${quarantinePath}-${process.pid}`
  }
  fs.renameSync(cachePath, quarantinePath)
  return quarantinePath
}

function readJson(filePath: string): any {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'))
}

function writeBlob(cachePath: string, content: string | Buffer): string {
  const buffer = Buffer.isBuffer(content) ? content : Buffer.from(content)
  const digest = crypto.createHash('sha256').update(buffer).digest('hex')
  const blobPath = path.join(cachePath, 'blobs', 'sha256', digest)
  fs.mkdirSync(path.dirname(blobPath), { recursive: true })
  fs.writeFileSync(blobPath, buffer)
  return `sha256:${digest}`
}

function runSelfTest() {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'rustpanel-buildkit-cache-test-'))
  try {
    const cachePath = path.join(tempRoot, 'cache')
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

    const validResult = validateLocalCache(cachePath)
    if (!validResult.ok) {
      throw new Error(`expected valid cache, got: ${validResult.reason}`)
    }

    const layerBlobPath = blobPathForDigest(cachePath, layerDigest)
    if (!layerBlobPath) {
      throw new Error('expected test layer digest to resolve to a blob path')
    }
    fs.rmSync(layerBlobPath, { force: true })
    const invalidResult = validateLocalCache(cachePath)
    if (invalidResult.ok || !invalidResult.reason.includes('missing blob')) {
      throw new Error(`expected missing blob failure, got: ${JSON.stringify(invalidResult)}`)
    }

    console.log('BuildKit local cache validator self-test passed.')
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true })
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function fail(message: string): never {
  console.error(message)
  process.exit(1)
}
