#!/usr/bin/env bash
set -euo pipefail

# RustPanel 卸载脚本
# 默认只停止服务并删除二进制、systemd 单元;数据目录默认保留。
# --purge 才会删除 /www/wwwroot/rustpanel 整个目录(含 .env、数据)。

INSTALL_DIR="${RUSTPANEL_INSTALL_DIR:-/www/wwwroot/rustpanel}"
DATA_DIR="${RUSTPANEL_DATA_DIR:-$INSTALL_DIR/data}"
COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-rustpanel}"
PURGE=0
KEEP_DATA=1
ASSUME_YES=0

usage() {
  cat <<'USAGE'
RustPanel uninstaller

Removes RustPanel from this host. By default keeps the data directory and
.env file so a future reinstall can recover state. Pass --purge to wipe
everything including admin password and audit logs.

Usage:
  bash uninstall.sh [options]

Options:
  --install-dir DIR   Install directory to clean up (default: /www/wwwroot/rustpanel)
  --data-dir DIR      Data directory (default: <install-dir>/data)
  --purge             Also delete install dir AND data dir (DESTRUCTIVE, includes audit logs)
  --keep-data         Keep data dir even with --purge (default behavior without --purge)
  --yes               Skip confirmation prompts
  -h, --help          Show this help
USAGE
}

log() { printf '[RustPanel uninstall] %s\n' "$*"; }
warn() { printf '[RustPanel uninstall] WARN: %s\n' "$*" >&2; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-dir) INSTALL_DIR="${2:?--install-dir requires value}"; shift 2 ;;
    --data-dir) DATA_DIR="${2:?--data-dir requires value}"; shift 2 ;;
    --purge) PURGE=1; KEEP_DATA=0; shift ;;
    --keep-data) KEEP_DATA=1; shift ;;
    --yes|-y) ASSUME_YES=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'unknown option: %s\n' "$1" >&2; usage; exit 2 ;;
  esac
done

if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  printf 'please run as root, e.g. sudo bash uninstall.sh\n' >&2
  exit 1
fi

# 加载 .env 拿到准确的安装/数据目录,避免误删
if [[ -f "$INSTALL_DIR/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$INSTALL_DIR/.env"
  set +a
  INSTALL_DIR="${RUSTPANEL_INSTALL_DIR:-$INSTALL_DIR}"
  DATA_DIR="${RUSTPANEL_DATA_DIR:-$DATA_DIR}"
  COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-rustpanel}"
fi

INSTALL_MODE="${RUSTPANEL_INSTALL_MODE:-auto}"

# 即将执行的动作概览
printf '\n[RustPanel uninstall] Plan:\n'
printf '  Install dir:   %s%s\n' "$INSTALL_DIR" "$([[ $PURGE == 1 ]] && printf ' (WILL DELETE)' || printf ' (keep)')"
printf '  Data dir:      %s%s\n' "$DATA_DIR" "$([[ $PURGE == 1 && $KEEP_DATA == 0 ]] && printf ' (WILL DELETE — audit logs lost)' || printf ' (keep)')"
printf '  Install mode:  %s\n' "$INSTALL_MODE"
printf '\n'

if [[ "$ASSUME_YES" != "1" && -t 0 ]]; then
  printf '[RustPanel uninstall] Continue? [y/N] '
  read -r answer
  case "$answer" in
    y|Y|yes|YES) ;;
    *) log "cancelled"; exit 0 ;;
  esac
fi

# 1. 停止并禁用 systemd 服务
if command -v systemctl >/dev/null 2>&1; then
  if systemctl list-unit-files 2>/dev/null | grep -q '^rustpanel-backend\.service'; then
    log "stopping rustpanel-backend.service"
    systemctl stop rustpanel-backend.service 2>/dev/null || true
    systemctl disable rustpanel-backend.service 2>/dev/null || true
  fi
  if [[ -f /etc/systemd/system/rustpanel-backend.service ]]; then
    rm -f /etc/systemd/system/rustpanel-backend.service
    systemctl daemon-reload 2>/dev/null || true
  fi
fi

# 2. 杀掉 daemon 模式下未托管的进程
if [[ -f "$INSTALL_DIR/rustpanel.pid" ]]; then
  pid="$(cat "$INSTALL_DIR/rustpanel.pid" 2>/dev/null || true)"
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    log "killing daemon process pid=$pid"
    kill "$pid" 2>/dev/null || true
    sleep 1
    kill -9 "$pid" 2>/dev/null || true
  fi
  rm -f "$INSTALL_DIR/rustpanel.pid"
fi

# 兜底:按进程名再扫一遍
if pgrep -x rustpanel-backend >/dev/null 2>&1; then
  log "killing leftover rustpanel-backend processes"
  pkill -x rustpanel-backend 2>/dev/null || true
  sleep 1
  pkill -9 -x rustpanel-backend 2>/dev/null || true
fi

# 3. Docker 模式:停容器 + 清编排
if [[ "$INSTALL_MODE" == "docker" || -f "$INSTALL_DIR/deploy/docker-compose.ghcr.yml" ]]; then
  if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    if docker compose version >/dev/null 2>&1; then
      compose_file="$INSTALL_DIR/deploy/docker-compose.ghcr.yml"
      if [[ -f "$compose_file" ]]; then
        log "docker compose down"
        docker compose -f "$compose_file" -p "$COMPOSE_PROJECT_NAME" down --remove-orphans 2>/dev/null || true
      fi
    fi
    # 清残留容器
    leftover="$(docker ps -aq --filter "name=^${COMPOSE_PROJECT_NAME}-" 2>/dev/null || true)"
    if [[ -n "$leftover" ]]; then
      log "removing leftover containers"
      # shellcheck disable=SC2086
      docker rm -f $leftover 2>/dev/null || true
    fi
  fi
fi

# 4. 删除二进制(默认行为)
if [[ -d "$INSTALL_DIR/bin" ]]; then
  log "removing binaries in $INSTALL_DIR/bin"
  rm -rf "$INSTALL_DIR/bin"
fi

# 5. 按需删除安装/数据目录
if [[ "$PURGE" == "1" ]]; then
  if [[ "$KEEP_DATA" == "1" && -d "$DATA_DIR" ]]; then
    log "purging install dir but keeping data dir"
    # 把 data dir 暂时挪走
    backup_data="/tmp/rustpanel-data-$(date +%s)"
    mv "$DATA_DIR" "$backup_data"
    rm -rf "$INSTALL_DIR"
    mkdir -p "$(dirname "$DATA_DIR")"
    mv "$backup_data" "$DATA_DIR"
    log "data preserved at $DATA_DIR"
  elif [[ -d "$INSTALL_DIR" ]]; then
    log "purging install dir AND data dir"
    rm -rf "$INSTALL_DIR"
    if [[ "$DATA_DIR" != "$INSTALL_DIR"* && -d "$DATA_DIR" ]]; then
      rm -rf "$DATA_DIR"
    fi
  fi
else
  warn "install dir preserved at $INSTALL_DIR (use --purge to remove)"
  warn ".env still contains admin password and JWT secret"
fi

log "uninstall complete"
