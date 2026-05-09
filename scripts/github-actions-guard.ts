#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'

const workflowsDir = path.resolve('.github/workflows')
const backendWorkflowPath = path.join(workflowsDir, 'backend.yml')
const dockerfilePath = path.resolve('src/backend/Dockerfile')
const issues: string[] = []

if (fs.existsSync(workflowsDir)) {
  for (const fileName of fs.readdirSync(workflowsDir)) {
    if (!/\.ya?ml$/.test(fileName)) continue
    const content = fs.readFileSync(path.join(workflowsDir, fileName), 'utf8')
    if (!content.includes('FORCE_JAVASCRIPT_ACTIONS_TO_NODE24')) {
      issues.push(`${fileName}: missing FORCE_JAVASCRIPT_ACTIONS_TO_NODE24`)
    }
    if (/setup-node@v[1-5]/.test(content)) {
      issues.push(`${fileName}: setup-node must support Node 24`)
    }
    if (/node\s+scripts\/[^\s]+\.cjs/.test(content)) {
      issues.push(`${fileName}: run compiled TypeScript scripts from dist/node-scripts, not scripts/*.cjs`)
    }
    if (content.includes('docker/build-push-action@')) {
      if (!content.includes('cache-from:') || !content.includes('cache-to:')) {
        issues.push(`${fileName}: docker/build-push-action must configure cache-from and cache-to`)
      }
      if (!content.includes('builder:')) {
        issues.push(`${fileName}: docker/build-push-action must use an explicit builder`)
      }
    }
    if (content.includes('docker/setup-buildx-action@')) {
      if (!content.includes('keep-state: true') || !content.includes('cleanup: false')) {
        issues.push(`${fileName}: docker/setup-buildx-action must keep self-hosted BuildKit state`)
      }
    }
  }
}

if (fs.existsSync(backendWorkflowPath)) {
  const backend = fs.readFileSync(backendWorkflowPath, 'utf8')
  for (const required of [
    "BUILDKIT_CACHE_ROOT: ${{ vars.BUILDKIT_CACHE_ROOT || '/cache/buildkit' }}",
    'BACKEND_IMAGE_CACHE_FAMILY: rust-backend-linux-amd64-v1',
    'BUILDKIT_CACHE_PROJECT: rustpanel',
    'concurrency:',
    'scripts/warm-docker-base-images.sh src/backend/Dockerfile',
    'id: image_ref',
    'owner_lc="${GITHUB_REPOSITORY_OWNER,,}"',
    'name: Collect backend image cache imports',
    'dist/node-scripts/scripts/collect-buildkit-cache-imports.js',
    '--include-legacy-root-peers',
    'REGISTRY_CACHE: ${{ steps.image_ref.outputs.image_repository }}:buildcache',
    '${{ steps.image_ref.outputs.image_repository }}:latest',
    '${{ steps.image_ref.outputs.image_repository }}:sha-${{ github.sha }}',
    'cache-from: ${{ steps.backend-cache.outputs.cache-from }}',
    'cache-to: ${{ steps.backend-cache.outputs.cache-to }}',
    '${{ env.BACKEND_IMAGE_CACHE_NAME }}-next-${{ github.run_id }}',
  ]) {
    if (!backend.includes(required)) {
      issues.push(`backend.yml: missing required cache marker "${required}"`)
    }
  }
}

if (fs.existsSync(dockerfilePath)) {
  const dockerfile = fs.readFileSync(dockerfilePath, 'utf8')
  for (const required of [
    '# syntax=docker/dockerfile:1.7',
    'cargo install sccache --version 0.15.0 --locked',
    'cargo install cargo-chef --locked',
    'FROM chef AS planner',
    'FROM chef AS ci-deps',
    'cargo chef cook --locked --all-targets --recipe-path recipe.json',
    'cargo chef cook --release --locked --recipe-path recipe.json',
    'id=rust-sccache-v1',
    'id=rust-cargo-registry-v1',
    'id=rust-cargo-git-v1',
    'id=rustpanel-backend-ci-target-v2',
    'sccache --show-stats',
  ]) {
    if (!dockerfile.includes(required)) {
      issues.push(`src/backend/Dockerfile: missing required cache marker "${required}"`)
    }
  }
}

if (issues.length > 0) {
  console.error(issues.join('\n'))
  process.exit(1)
}

console.log('GitHub Actions guard passed.')
