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
    if (content.includes('oven-sh/setup-bun@')) {
      issues.push(`${fileName}: self-hosted workflows must use the preinstalled runner Bun instead of oven-sh/setup-bun`)
    }
    if (content.includes('bun ') && !content.includes('bun --revision')) {
      issues.push(`${fileName}: workflow must verify the preinstalled runner Bun before using bun commands`)
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
    // 发布链路只用 runner 自带的 bun 跑脚本,不装 node(action 下载是单点故障)。
    'bun dist/node-scripts/scripts/validate-buildkit-local-cache.js',
    'bun dist/node-scripts/scripts/collect-buildkit-cache-imports.js',
    'bun dist/node-scripts/scripts/publish-micro-release.js',
    '--include-legacy-root-peers',
    'REGISTRY_CACHE: ${{ steps.image_ref.outputs.image_repository }}:buildcache',
    '${{ steps.image_ref.outputs.image_repository }}:latest',
    '${{ steps.image_ref.outputs.image_repository }}:sha-${{ github.sha }}',
    'cache-from: ${{ steps.backend-cache.outputs.cache-from }}',
    'cache-to: ${{ steps.backend-cache.outputs.cache-to }}',
    '${{ env.BACKEND_IMAGE_CACHE_NAME }}-next-${{ github.run_id }}',
    'dist/node-scripts/scripts/publish-micro-release.js',
    'GITHUB_TOKEN: ${{ github.token }}',
    // 三份后端产物靠 runner 本地目录交接(load: true 后从本地镜像里取 full 二进制),
    // 不走 docker pull 自己刚推的镜像,也不走 artifact 上传/下载:
    // runner ↔ ghcr.io / GitHub blob storage 一抖,整条发布就卡死。
    'RELEASE_ARTIFACT_ROOT: ${{ vars.RELEASE_ARTIFACT_ROOT || \'/cache/rustpanel-artifacts\' }}',
    'name: Export full backend binary',
    'name: Publish full backend binary',
    'name: Publish micro backend binary',
    'name: Publish musl backend binary',
    'name: Collect backend binaries',
  ]) {
    if (!backend.includes(required)) {
      issues.push(`backend.yml: missing required cache marker "${required}"`)
    }
  }
  if (/gh\s+release/.test(backend)) {
    issues.push('backend.yml: publish release step must not depend on gh CLI')
  }
  if (/docker pull --platform/.test(backend)) {
    issues.push(
      'backend.yml: publish step must not docker pull from the registry; hand the full binary over through RELEASE_ARTIFACT_ROOT instead',
    )
  }
  // 产物交接不许再走 artifact:上传/下载都要过 GitHub blob storage,
  // self-hosted runner 出网抖动时会把发布整条带挂(见 43798143654 / 43789347523)。
  if (/actions\/(upload|download)-artifact/.test(backend)) {
    issues.push(
      'backend.yml: must not use upload/download-artifact; hand backend binaries over through RELEASE_ARTIFACT_ROOT',
    )
  }
  // 发布 job 不装 node:setup-node 要从 codeload.github.com 下 action,
  // runner 出网一抖 job 就卡在 Setup(见 43801216199)。只允许 verify job 用一次。
  if ((backend.match(/actions\/setup-node/g) ?? []).length > 1) {
    issues.push(
      'backend.yml: only the verify job may use actions/setup-node; release jobs must run on the runner bun',
    )
  }
}

if (fs.existsSync(dockerfilePath)) {
  const dockerfile = fs.readFileSync(dockerfilePath, 'utf8')
  // 这两行的版本号由 Renovate 的 dockerfile manager 自动升级,只认结构不认版本号,
  // 不然每次 Renovate 提的基础镜像升级 PR 都会被这个 guard 拦下来。
  if (!/^# syntax=docker\/dockerfile:[\d.]+$/m.test(dockerfile)) {
    issues.push('src/backend/Dockerfile: missing required cache marker "# syntax=docker/dockerfile:<version>"')
  }
  // web 构建必须钉在 BUILDPLATFORM 上:dist 与目标架构无关,跟着 linux/amd64 走 QEMU
  // 模拟时 Bun 会以 MemoryExhaustion abort,把三个发布 job 全带挂(见 d871cda)。
  if (!/^FROM --platform=\$BUILDPLATFORM oven\/bun:[\d.]+ AS web-builder$/m.test(dockerfile)) {
    issues.push(
      'src/backend/Dockerfile: missing required cache marker "FROM --platform=$BUILDPLATFORM oven/bun:<version> AS web-builder"',
    )
  }
  for (const required of [
    'bun install --frozen-lockfile',
    'bun run build',
    'COPY --from=web-builder /app/src/web/dist /app/src/web/dist',
    'cargo install sccache --version 0.15.0 --locked',
    'cargo install cargo-chef --locked',
    'FROM chef AS planner',
    'FROM chef AS ci-deps',
    'cargo chef cook --locked --all-targets --recipe-path recipe.json',
    // 交叉编译走 cargo-zigbuild(见 Dockerfile 注释),不再用 chef cook --release 那一步
    'cargo zigbuild --release --locked --bins --target',
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
