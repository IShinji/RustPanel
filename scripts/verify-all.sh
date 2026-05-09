#!/usr/bin/env bash
set -euo pipefail

root_dir="$(git rev-parse --show-toplevel)"
cd "$root_dir"

bun install --frozen-lockfile
bun run scripts:check
bun run scripts:build
if [[ -f buf.yaml ]]; then
  bun run proto:lint
  bun run proto:generate
  git diff --exit-code -- src/web/src/gen
fi
node --check dist/node-scripts/scripts/check-latest-ci.js
node --check dist/node-scripts/scripts/cleanup-ghcr-package-versions.js
node --check dist/node-scripts/scripts/collect-buildkit-cache-imports.js
node --check dist/node-scripts/scripts/github-actions-guard.js
node --check dist/node-scripts/scripts/publish-micro-release.js
node --check dist/node-scripts/scripts/sync-release-version.js
node --check dist/node-scripts/scripts/validate-buildkit-local-cache.js
node dist/node-scripts/scripts/check-latest-ci.js --self-test
node dist/node-scripts/scripts/cleanup-ghcr-package-versions.js --self-test
node dist/node-scripts/scripts/publish-micro-release.js --self-test
node dist/node-scripts/scripts/sync-release-version.js --check
node dist/node-scripts/scripts/validate-buildkit-local-cache.js --self-test
node dist/node-scripts/scripts/collect-buildkit-cache-imports.js --self-test
node dist/node-scripts/scripts/github-actions-guard.js
bash -n scripts/warm-docker-base-images.sh

if [[ -f src/backend/Cargo.toml ]]; then
  cd "$root_dir/src/backend"
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo test --all-targets -- --quiet
fi

if [[ -f "$root_dir/src/web/package.json" ]]; then
  cd "$root_dir/src/web"
  bun install --frozen-lockfile
  bun lint
  bun test
  bun run build
fi

if [[ -f "$root_dir/src/admin/package.json" ]]; then
  cd "$root_dir/src/admin"
  bun install --frozen-lockfile
  bun lint
  bun test
  bun run build
fi
