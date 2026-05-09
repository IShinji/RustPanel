#!/usr/bin/env bash
set -euo pipefail

REPO_RAW_BASE="${RUSTPANEL_RAW_BASE:-https://raw.githubusercontent.com/IShinji/RustPanel/main}"
INSTALL_DIR="${RUSTPANEL_INSTALL_DIR:-/www/wwwroot/rustpanel}"
DATA_DIR="${RUSTPANEL_DATA_DIR:-$INSTALL_DIR/data}"
DATA_DIR_EXPLICIT=0
BACKEND_IMAGE="${BACKEND_IMAGE:-ghcr.io/ishinji/rustpanel-backend:latest}"
RUSTPANEL_API_PORT="${RUSTPANEL_API_PORT:-18080}"
RUSTPANEL_BIND_HOST="${RUSTPANEL_BIND_HOST:-0.0.0.0}"
RUSTPANEL_ALLOWED_ORIGINS="${RUSTPANEL_ALLOWED_ORIGINS:-}"
RUSTPANEL_ADMIN_USERNAME="${RUSTPANEL_ADMIN_USERNAME:-admin}"
RUSTPANEL_ADMIN_PASSWORD="${RUSTPANEL_ADMIN_PASSWORD:-}"
RUSTPANEL_JWT_SECRET="${RUSTPANEL_JWT_SECRET:-}"
COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-rustpanel}"
GITHUB_TOKEN="${GITHUB_TOKEN:-}"
GHCR_USERNAME="${GHCR_USERNAME:-_}"
SKIP_DOCKER_INSTALL="${RUSTPANEL_SKIP_DOCKER_INSTALL:-0}"
FORCE="${RUSTPANEL_INSTALL_FORCE:-0}"

usage() {
  cat <<'USAGE'
RustPanel one-click installer

Usage:
  bash install.sh [options]

Options:
  --install-dir DIR        Install files to DIR (default: /www/wwwroot/rustpanel)
  --data-dir DIR           Persist RustPanel data in DIR (default: <install-dir>/data)
  --port PORT              Host port for the panel (default: 18080)
  --bind HOST              Host bind address (default: 0.0.0.0)
  --image IMAGE            Backend image (default: ghcr.io/ishinji/rustpanel-backend:latest)
  --origin ORIGIN          Allowed browser origin, for example https://panel.example.com
  --admin-username USER    Admin username (default: admin)
  --admin-password PASS    Admin password (default: generated)
  --github-token TOKEN     Token for private GHCR image pulls
  --ghcr-username USER     GHCR login username when --github-token is set
  --skip-docker-install    Require existing Docker instead of installing it
  --force                  Rewrite an existing .env with the provided/generated values
  -h, --help               Show this help

Environment variables with the same names are also supported, for example:
  RUSTPANEL_API_PORT=18080 RUSTPANEL_ALLOWED_ORIGINS=https://panel.example.com bash install.sh
USAGE
}

log() {
  printf '[RustPanel] %s\n' "$*"
}

fail() {
  printf '[RustPanel] ERROR: %s\n' "$*" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-dir)
      INSTALL_DIR="${2:?missing value for --install-dir}"
      if [[ "$DATA_DIR_EXPLICIT" != "1" ]]; then
        DATA_DIR="$INSTALL_DIR/data"
      fi
      shift 2
      ;;
    --data-dir)
      DATA_DIR="${2:?missing value for --data-dir}"
      DATA_DIR_EXPLICIT=1
      shift 2
      ;;
    --port)
      RUSTPANEL_API_PORT="${2:?missing value for --port}"
      shift 2
      ;;
    --bind)
      RUSTPANEL_BIND_HOST="${2:?missing value for --bind}"
      shift 2
      ;;
    --image)
      BACKEND_IMAGE="${2:?missing value for --image}"
      shift 2
      ;;
    --origin)
      RUSTPANEL_ALLOWED_ORIGINS="${2:?missing value for --origin}"
      shift 2
      ;;
    --admin-username)
      RUSTPANEL_ADMIN_USERNAME="${2:?missing value for --admin-username}"
      shift 2
      ;;
    --admin-password)
      RUSTPANEL_ADMIN_PASSWORD="${2:?missing value for --admin-password}"
      shift 2
      ;;
    --github-token)
      GITHUB_TOKEN="${2:?missing value for --github-token}"
      shift 2
      ;;
    --ghcr-username)
      GHCR_USERNAME="${2:?missing value for --ghcr-username}"
      shift 2
      ;;
    --skip-docker-install)
      SKIP_DOCKER_INSTALL=1
      shift
      ;;
    --force)
      FORCE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown option: $1"
      ;;
  esac
done

if [[ "$(uname -s)" != "Linux" ]]; then
  fail "RustPanel one-click install currently supports Linux hosts only"
fi

if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  fail "please run as root, for example: sudo bash install.sh"
fi

case "$RUSTPANEL_API_PORT" in
  ''|*[!0-9]*)
    fail "--port must be a number"
    ;;
esac

if (( RUSTPANEL_API_PORT < 1 || RUSTPANEL_API_PORT > 65535 )); then
  fail "--port must be between 1 and 65535"
fi

if [[ -f "$INSTALL_DIR/.env" && "$FORCE" != "1" ]]; then
  log "Loading existing configuration from $INSTALL_DIR/.env"
  set -a
  # shellcheck disable=SC1091
  source "$INSTALL_DIR/.env"
  set +a
  INSTALL_DIR="${RUSTPANEL_INSTALL_DIR:-$INSTALL_DIR}"
  DATA_DIR="${RUSTPANEL_DATA_DIR:-$DATA_DIR}"
  BACKEND_IMAGE="${BACKEND_IMAGE:-ghcr.io/ishinji/rustpanel-backend:latest}"
  RUSTPANEL_API_PORT="${RUSTPANEL_API_PORT:-18080}"
  RUSTPANEL_BIND_HOST="${RUSTPANEL_BIND_HOST:-0.0.0.0}"
  RUSTPANEL_ALLOWED_ORIGINS="${RUSTPANEL_ALLOWED_ORIGINS:-}"
  RUSTPANEL_ADMIN_USERNAME="${RUSTPANEL_ADMIN_USERNAME:-admin}"
  RUSTPANEL_ADMIN_PASSWORD="${RUSTPANEL_ADMIN_PASSWORD:-}"
  RUSTPANEL_JWT_SECRET="${RUSTPANEL_JWT_SECRET:-}"
  COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-rustpanel}"
  GITHUB_TOKEN="${GITHUB_TOKEN:-}"
  GHCR_USERNAME="${GHCR_USERNAME:-_}"
fi

random_hex() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex "${1:-32}"
    return
  fi

  if command -v od >/dev/null 2>&1; then
    od -An -N"${1:-32}" -tx1 /dev/urandom | tr -d ' \n'
    return
  fi

  fail "openssl or od is required to generate secrets"
}

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

  fail "curl or wget is required"
}

install_docker() {
  if command -v docker >/dev/null 2>&1; then
    return
  fi

  if [[ "$SKIP_DOCKER_INSTALL" == "1" ]]; then
    fail "Docker is not installed and --skip-docker-install was set"
  fi

  log "Docker not found, installing Docker Engine"
  local docker_install="/tmp/rustpanel-get-docker.sh"
  download_file "https://get.docker.com" "$docker_install"
  sh "$docker_install"
}

ensure_compose_plugin() {
  if docker compose version >/dev/null 2>&1; then
    return
  fi

  log "Docker Compose plugin not found, trying package manager install"
  if command -v apt-get >/dev/null 2>&1; then
    apt-get update
    apt-get install -y docker-compose-plugin
  elif command -v dnf >/dev/null 2>&1; then
    dnf install -y docker-compose-plugin
  elif command -v yum >/dev/null 2>&1; then
    yum install -y docker-compose-plugin
  else
    fail "Docker Compose plugin is required; install docker compose and rerun"
  fi

  docker compose version >/dev/null 2>&1 || fail "Docker Compose plugin is still unavailable"
}

start_docker() {
  if command -v systemctl >/dev/null 2>&1; then
    systemctl enable --now docker >/dev/null 2>&1 || true
  elif command -v service >/dev/null 2>&1; then
    service docker start >/dev/null 2>&1 || true
  fi

  docker info >/dev/null 2>&1 || fail "Docker daemon is not running"
}

public_ip() {
  hostname -I 2>/dev/null | awk '{print $1}'
}

write_env_var() {
  local key="$1"
  local value="$2"

  printf "%s='" "$key"
  printf "%s" "$value" | sed "s/'/'\\\\''/g"
  printf "'\n"
}

RUSTPANEL_ADMIN_PASSWORD="${RUSTPANEL_ADMIN_PASSWORD:-$(random_hex 12)}"
RUSTPANEL_JWT_SECRET="${RUSTPANEL_JWT_SECRET:-$(random_hex 32)}"

log "Preparing install directory: $INSTALL_DIR"
install -d -m 0755 "$INSTALL_DIR/deploy"
install -d -m 0700 "$DATA_DIR"

log "Downloading deployment files"
download_file "$REPO_RAW_BASE/deploy/docker-compose.ghcr.yml" "$INSTALL_DIR/deploy/docker-compose.ghcr.yml"
download_file "$REPO_RAW_BASE/deploy/update.sh" "$INSTALL_DIR/deploy/update.sh"
chmod +x "$INSTALL_DIR/deploy/update.sh"

log "Writing production configuration"
umask 077
{
  write_env_var "COMPOSE_PROJECT_NAME" "$COMPOSE_PROJECT_NAME"
  write_env_var "BACKEND_IMAGE" "$BACKEND_IMAGE"
  write_env_var "RUSTPANEL_INSTALL_DIR" "$INSTALL_DIR"
  write_env_var "RUSTPANEL_DATA_DIR" "$DATA_DIR"
  write_env_var "RUSTPANEL_BIND_HOST" "$RUSTPANEL_BIND_HOST"
  write_env_var "RUSTPANEL_API_PORT" "$RUSTPANEL_API_PORT"
  write_env_var "RUSTPANEL_ALLOWED_ORIGINS" "$RUSTPANEL_ALLOWED_ORIGINS"
  write_env_var "RUSTPANEL_ADMIN_USERNAME" "$RUSTPANEL_ADMIN_USERNAME"
  write_env_var "RUSTPANEL_ADMIN_PASSWORD" "$RUSTPANEL_ADMIN_PASSWORD"
  write_env_var "RUSTPANEL_JWT_SECRET" "$RUSTPANEL_JWT_SECRET"
  write_env_var "GITHUB_TOKEN" "$GITHUB_TOKEN"
  write_env_var "GHCR_USERNAME" "$GHCR_USERNAME"
} > "$INSTALL_DIR/.env"

install_docker
ensure_compose_plugin
start_docker

log "Starting RustPanel"
bash "$INSTALL_DIR/deploy/update.sh"

host="${RUSTPANEL_PUBLIC_HOST:-$(public_ip)}"
if [[ -z "$host" || "$RUSTPANEL_BIND_HOST" == "127.0.0.1" || "$RUSTPANEL_BIND_HOST" == "localhost" ]]; then
  host="$RUSTPANEL_BIND_HOST"
fi

cat <<EOF

RustPanel installed successfully.

Panel URL: http://$host:$RUSTPANEL_API_PORT
Username:  $RUSTPANEL_ADMIN_USERNAME
Password:  $RUSTPANEL_ADMIN_PASSWORD

Install dir: $INSTALL_DIR
Data dir:    $DATA_DIR
Update:      bash $INSTALL_DIR/deploy/update.sh

Keep $INSTALL_DIR/.env private. It contains the admin password and JWT secret.
EOF
