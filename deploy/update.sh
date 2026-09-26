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

FULL_BINARY_URL="https://github.com/IShinji/RustPanel/releases/download/micro-latest/rustpanel-backend-linux-amd64.tar.gz"
MICRO_BINARY_URL="https://github.com/IShinji/RustPanel/releases/download/micro-latest/rustpanel-backend-micro-linux-amd64.tar.gz"

if [[ "${RUSTPANEL_INSTALL_MODE:-docker}" == "binary" ]]; then
  # 老 micro 安装还指着完整版二进制:切到精简构建并写回 .env
  if [[ "${RUSTPANEL_INSTALL_PROFILE:-}" == "micro" && "${RUSTPANEL_BINARY_URL:-}" == "$FULL_BINARY_URL" ]]; then
    RUSTPANEL_BINARY_URL="$MICRO_BINARY_URL"
    sed -i "s|^RUSTPANEL_BINARY_URL=.*|RUSTPANEL_BINARY_URL='$MICRO_BINARY_URL'|" "$PROJECT_ROOT/.env"
  fi
  archive="/tmp/rustpanel-backend.tar.gz"
  trap 'rm -f "$archive"' EXIT
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
    # 升级时同步刷新 systemd 单元(节俭模式 socket / 证书续签 timer),老安装也能用上。
    units="$PROJECT_ROOT/deploy/systemd-units.sh"
    units_tmp="$units.download"
    if download_file "${RUSTPANEL_RAW_BASE:-https://raw.githubusercontent.com/IShinji/RustPanel/main}/deploy/systemd-units.sh" "$units_tmp"; then
      mv "$units_tmp" "$units"
    else
      rm -f "$units_tmp"
    fi
    if [[ -f "$units" ]]; then
      # shellcheck disable=SC1090
      source "$units"
      env_file="$PROJECT_ROOT/.env"
      if [[ -z "${RUSTPANEL_FRUGAL:-}" ]]; then
        RUSTPANEL_FRUGAL=0
        [[ "${RUSTPANEL_INSTALL_PROFILE:-}" == "micro" ]] && RUSTPANEL_FRUGAL=1
        rustpanel_ensure_env_var "$env_file" RUSTPANEL_FRUGAL "$RUSTPANEL_FRUGAL"
      fi
      if [[ "$RUSTPANEL_FRUGAL" == "1" ]]; then
        rustpanel_ensure_env_var "$env_file" RUSTPANEL_IDLE_EXIT_MINUTES 10
      fi
      rustpanel_ensure_env_var "$env_file" RUSTPANEL_ENV_FILE "$env_file"
      if [[ -d /etc/cron.d ]]; then
        rustpanel_ensure_env_var "$env_file" RUSTPANEL_SYSTEM_CRONTAB /etc/cron.d/rustpanel
      fi
      INSTALL_DIR="$PROJECT_ROOT"
      RUSTPANEL_BIND_HOST="${RUSTPANEL_BIND_HOST:-0.0.0.0}"
      RUSTPANEL_API_PORT="${RUSTPANEL_API_PORT:-18080}"
      # 内部先 stop 再 enable --now,新二进制随之生效
      rustpanel_write_units
      if [[ "${RUSTPANEL_INSTALL_PROFILE:-}" == "micro" ]]; then
        rustpanel_apply_small_disk_tweaks
      fi
    else
      systemctl restart rustpanel-backend
    fi
  else
    if [[ -f "$PROJECT_ROOT/rustpanel.pid" ]]; then
      kill "$(cat "$PROJECT_ROOT/rustpanel.pid")" >/dev/null 2>&1 || true
    fi
    export MALLOC_ARENA_MAX="${MALLOC_ARENA_MAX:-2}"
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
