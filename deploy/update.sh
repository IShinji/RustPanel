#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ -f "$PROJECT_ROOT/.env" ]]; then
  set -a
  source "$PROJECT_ROOT/.env"
  set +a
fi

COMPOSE_FILE="${COMPOSE_FILE:-$PROJECT_ROOT/deploy/docker-compose.ghcr.yml}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:-${COMPOSE_PROJECT_NAME:-rustpanel}}"

download_file() {
  local url="$1"
  local target="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$target"
    return
  fi

  if command -v wget >/dev/null 2>&1; then
    wget -qO "$target" "$url"
    return
  fi

  echo "curl or wget is required" >&2
  exit 1
}

if [[ "${RUSTPANEL_INSTALL_MODE:-docker}" == "binary" ]]; then
  archive="/tmp/rustpanel-backend.tar.gz"
  bin_dir="$PROJECT_ROOT/bin"
  mkdir -p "$bin_dir"
  download_file "${RUSTPANEL_BINARY_URL:?RUSTPANEL_BINARY_URL is required}" "$archive"
  tar -xzf "$archive" -C "$bin_dir"
  if [[ ! -x "$bin_dir/rustpanel-backend" ]]; then
    found="$(find "$bin_dir" -type f -name rustpanel-backend -perm -111 | head -n 1)"
    [[ -n "$found" ]] || {
      echo "rustpanel-backend binary not found in archive" >&2
      exit 1
    }
    cp "$found" "$bin_dir/rustpanel-backend"
  fi
  chmod +x "$bin_dir/rustpanel-backend"
  if command -v systemctl >/dev/null 2>&1; then
    systemctl restart rustpanel-backend
  else
    if [[ -f "$PROJECT_ROOT/rustpanel.pid" ]]; then
      kill "$(cat "$PROJECT_ROOT/rustpanel.pid")" >/dev/null 2>&1 || true
    fi
    set -a
    # shellcheck disable=SC1091
    source "$PROJECT_ROOT/.env"
    set +a
    "$bin_dir/rustpanel-backend" --daemon
  fi
  exit 0
fi

if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  printf '%s' "$GITHUB_TOKEN" | docker login ghcr.io -u "${GHCR_USERNAME:-_}" --password-stdin
fi

docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT" pull backend
docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT" up -d backend
