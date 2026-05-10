#!/usr/bin/env bash
set -euo pipefail

REPO_RAW_BASE="${RUSTPANEL_RAW_BASE:-https://raw.githubusercontent.com/IShinji/RustPanel/main}"
INSTALL_DIR="${RUSTPANEL_INSTALL_DIR:-/www/wwwroot/rustpanel}"
DATA_DIR="${RUSTPANEL_DATA_DIR:-$INSTALL_DIR/data}"
DATA_DIR_EXPLICIT=0
BACKEND_IMAGE="${BACKEND_IMAGE:-ghcr.io/ishinji/rustpanel-backend:latest}"
BINARY_URL="${RUSTPANEL_BINARY_URL:-https://github.com/IShinji/RustPanel/releases/download/micro-latest/rustpanel-backend-linux-amd64.tar.gz}"
RUSTPANEL_API_PORT="${RUSTPANEL_API_PORT:-18080}"
RUSTPANEL_BIND_HOST="${RUSTPANEL_BIND_HOST:-0.0.0.0}"
RUSTPANEL_ALLOWED_ORIGINS="${RUSTPANEL_ALLOWED_ORIGINS:-}"
RUSTPANEL_ADMIN_USERNAME="${RUSTPANEL_ADMIN_USERNAME:-admin}"
RUSTPANEL_ADMIN_PASSWORD="${RUSTPANEL_ADMIN_PASSWORD:-}"
RUSTPANEL_JWT_SECRET="${RUSTPANEL_JWT_SECRET:-}"
RUSTPANEL_INSTALL_PROFILE="${RUSTPANEL_INSTALL_PROFILE:-auto}"
RUSTPANEL_INSTALL_MODE="${RUSTPANEL_INSTALL_MODE:-auto}"
RUSTPANEL_ENABLED_MODULES="${RUSTPANEL_ENABLED_MODULES:-}"
RUSTPANEL_DISABLED_MODULES="${RUSTPANEL_DISABLED_MODULES:-}"
RUSTPANEL_NAT_PORT_RANGE="${RUSTPANEL_NAT_PORT_RANGE:-}"
RUSTPANEL_PUBLIC_HOST="${RUSTPANEL_PUBLIC_HOST:-}"
RUSTPANEL_ULTRA_LOW="${RUSTPANEL_ULTRA_LOW:-0}"
COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-rustpanel}"
GITHUB_TOKEN="${GITHUB_TOKEN:-}"
GHCR_USERNAME="${GHCR_USERNAME:-_}"
SKIP_DOCKER_INSTALL="${RUSTPANEL_SKIP_DOCKER_INSTALL:-0}"
ASSUME_RECOMMENDED="${RUSTPANEL_ASSUME_RECOMMENDED:-0}"
DRY_RUN="${RUSTPANEL_DRY_RUN:-0}"
FORCE="${RUSTPANEL_INSTALL_FORCE:-0}"

# 资源硬门禁阈值,低于此线安装注定失败,提前 fail 给用户清理建议
MIN_DISK_MB=500
MIN_MEMORY_MB=80
NAT_PORT_LOW=""
NAT_PORT_HIGH=""

usage() {
  cat <<'USAGE'
RustPanel one-click installer

By default this installer runs in INTERACTIVE mode and prompts for the
NAT port range, the panel access port, the public IP/hostname, the admin
credentials, and an ultra-low-memory toggle. Pass --assume-recommended (or
pipe input via stdin) to skip all prompts and use detected/CLI values.

Usage:
  bash install.sh [options]

Options:
  --install-dir DIR        Install files to DIR (default: /www/wwwroot/rustpanel)
  --data-dir DIR           Persist RustPanel data in DIR (default: <install-dir>/data)
  --port PORT              Host port for the panel (default: 18080)
  --bind HOST              Host bind address (default: 0.0.0.0)
  --public-host HOST       Public IP or hostname users will type in browser
  --nat-port-range RANGE   NAT-opened port range (e.g. 1200-1219); panel port must fall inside
  --image IMAGE            Backend image (default: ghcr.io/ishinji/rustpanel-backend:latest)
  --binary-url URL         Backend binary tar.gz URL for binary/micro mode
  --profile PROFILE        auto, micro, lite, standard, or full (default: auto)
  --install-mode MODE      auto, docker, or binary (default: auto)
  --modules LIST           Comma-separated enabled modules
  --disable-modules LIST   Comma-separated disabled modules
  --ultra-low              Disable proxy and workloads modules (recommended for <=128MB RAM)
  --origin ORIGIN          Allowed browser origin, for example https://panel.example.com
  --admin-username USER    Admin username (default: admin)
  --admin-password PASS    Admin password (default: generated)
  --github-token TOKEN     Token for private GHCR image pulls
  --ghcr-username USER     GHCR login username when --github-token is set
  --skip-docker-install    Require existing Docker instead of installing it
  --assume-recommended     Skip ALL prompts and use detected recommendations / CLI values
  --dry-run                Print detected recommendations without installing
  --force                  Rewrite an existing .env with the provided/generated values
  -h, --help               Show this help

Environment variables with the same names are also supported, for example:
  RUSTPANEL_API_PORT=1205 RUSTPANEL_NAT_PORT_RANGE=1200-1219 bash install.sh --assume-recommended
USAGE
}

log() {
  printf '[RustPanel] %s\n' "$*"
}

fail() {
  printf '[RustPanel] ERROR: %s\n' "$*" >&2
  exit 1
}

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

detect_memory_mb() {
  awk '/MemTotal:/ { printf "%d", $2 / 1024 }' /proc/meminfo 2>/dev/null || printf '0'
}

detect_disk_mb() {
  local target="$INSTALL_DIR"
  while [[ ! -d "$target" && "$target" != "/" ]]; do
    target="$(dirname "$target")"
  done
  df -Pm "$target" 2>/dev/null | awk 'NR==2 { print $4 }' || printf '0'
}

detect_virtualization() {
  if command_exists systemd-detect-virt; then
    local virt
    virt="$(systemd-detect-virt 2>/dev/null || true)"
    printf '%s\n' "${virt:-unknown}"
    return
  fi
  if [[ -d /proc/vz && ! -d /proc/bc ]]; then
    printf 'openvz\n'
    return
  fi
  if grep -qa openvz /proc/1/environ 2>/dev/null; then
    printf 'openvz\n'
    return
  fi
  printf 'unknown\n'
}

docker_ready() {
  command_exists docker && docker info >/dev/null 2>&1 && docker compose version >/dev/null 2>&1
}

# 探测主机首个非环回 IP;NAT 机上拿到的常是内网地址,仅作为默认值占位,需用户在交互里覆盖
public_ip() {
  hostname -I 2>/dev/null | awk '{print $1}'
}

# 校验端口字符串是否为合法 1-65535 整数,失败时返回非零并打印原因
validate_port_number() {
  local port="$1"
  case "$port" in
    ''|*[!0-9]*)
      printf 'port must be a number\n' >&2
      return 1
      ;;
  esac
  if (( port < 1 || port > 65535 )); then
    printf 'port must be between 1 and 65535\n' >&2
    return 1
  fi
}

# 解析形如 "1200-1219" 的 NAT 端口范围,设置 NAT_PORT_LOW / NAT_PORT_HIGH
# 空字符串视为无 NAT 限制,清空两个变量。非法格式 fail。
parse_nat_port_range() {
  local raw="$1"
  if [[ -z "$raw" ]]; then
    NAT_PORT_LOW=""
    NAT_PORT_HIGH=""
    return
  fi
  if [[ ! "$raw" =~ ^[0-9]+-[0-9]+$ ]]; then
    fail "NAT port range must be in the form LOW-HIGH (e.g. 1200-1219), got: $raw"
  fi
  local low="${raw%-*}"
  local high="${raw#*-}"
  validate_port_number "$low" || fail "invalid low port in NAT range: $low"
  validate_port_number "$high" || fail "invalid high port in NAT range: $high"
  if (( low > high )); then
    fail "NAT port range low ($low) must be <= high ($high)"
  fi
  NAT_PORT_LOW="$low"
  NAT_PORT_HIGH="$high"
}

# 端口是否在 NAT 放通段内;无范围只校验 1-65535
validate_port_against_nat() {
  local port="$1"
  validate_port_number "$port" || return 1
  if [[ -n "$NAT_PORT_LOW" && -n "$NAT_PORT_HIGH" ]]; then
    if (( port < NAT_PORT_LOW || port > NAT_PORT_HIGH )); then
      printf 'port %s is outside NAT range %s-%s\n' "$port" "$NAT_PORT_LOW" "$NAT_PORT_HIGH" >&2
      return 1
    fi
  fi
}

# 资源硬门禁:磁盘 / 内存低于阈值则提前 fail,避免装到一半 OOM 或塞满磁盘
preflight_resources() {
  local disk_mb memory_mb
  disk_mb="$(detect_disk_mb)"
  memory_mb="$(detect_memory_mb)"
  if [[ -n "$disk_mb" ]] && (( disk_mb < MIN_DISK_MB )); then
    fail "free disk space ${disk_mb}MB is below ${MIN_DISK_MB}MB; clean up with 'apt-get clean && journalctl --vacuum-size=50M' and retry"
  fi
  if [[ -n "$memory_mb" ]] && (( memory_mb > 0 && memory_mb < MIN_MEMORY_MB )); then
    fail "total memory ${memory_mb}MB is below ${MIN_MEMORY_MB}MB; RustPanel requires at least ${MIN_MEMORY_MB}MB"
  fi
}

recommended_profile() {
  local memory_mb="$1"
  local disk_mb="$2"
  local virt="$3"
  if (( memory_mb <= 256 || disk_mb <= 3072 )) || [[ "$virt" == "openvz" ]] || ! command_exists docker; then
    printf 'micro'
  elif (( memory_mb >= 2048 && disk_mb >= 20480 )) && docker_ready; then
    printf 'full'
  else
    printf 'standard'
  fi
}

modules_for_profile() {
  case "$1" in
    micro)
      # Phase B 后:appstore 已是 12 个轻量 native 包,sites 已 NAT/IPv6 感知,
      # ssl 已支持 DNS-01,security 在 OpenVZ 上 iptables 受限但其它能用,
      # 因此从 micro 默认开启列表里把这些加回来。docker / cluster 仍然不开。
      printf 'core,audit,monitor,files,terminal,security,appstore,sites,ssl,static-sites,workloads,proxy,cron,database'
      ;;
    lite)
      printf 'core,audit,monitor,files,terminal,security,appstore,sites,ssl,static-sites,cron,database'
      ;;
    standard)
      printf 'core,audit,monitor,files,terminal,security,docker,appstore,sites,ssl,database,cron'
      ;;
    full)
      printf 'core,audit,monitor,files,terminal,security,docker,appstore,sites,static-sites,ssl,database,cron,cluster,workloads,proxy'
      ;;
    *)
      printf 'core,audit,monitor,files,terminal,security,docker,appstore,sites,ssl,database,cron'
      ;;
  esac
}

disabled_modules_for_profile() {
  case "$1" in
    micro)
      # 只禁用真跑不动的:Docker(OpenVZ 内核多半缺 overlay2)、cluster(单机)
      printf 'docker,cluster'
      ;;
    lite)
      printf 'docker,cluster,workloads,proxy'
      ;;
    *)
      printf ''
      ;;
  esac
}

install_mode_for_profile() {
  case "$1" in
    micro|lite)
      printf 'binary'
      ;;
    *)
      printf 'docker'
      ;;
  esac
}

apply_recommendations() {
  DETECTED_MEMORY_MB="$(detect_memory_mb)"
  DETECTED_DISK_MB="$(detect_disk_mb)"
  DETECTED_VIRT="$(detect_virtualization)"
  RECOMMENDED_PROFILE="$(recommended_profile "$DETECTED_MEMORY_MB" "$DETECTED_DISK_MB" "$DETECTED_VIRT")"

  if [[ "$RUSTPANEL_INSTALL_PROFILE" == "auto" ]]; then
    RUSTPANEL_INSTALL_PROFILE="$RECOMMENDED_PROFILE"
  fi
  if [[ "$RUSTPANEL_INSTALL_MODE" == "auto" ]]; then
    RUSTPANEL_INSTALL_MODE="$(install_mode_for_profile "$RUSTPANEL_INSTALL_PROFILE")"
  fi
  if [[ -z "$RUSTPANEL_ENABLED_MODULES" ]]; then
    RUSTPANEL_ENABLED_MODULES="$(modules_for_profile "$RUSTPANEL_INSTALL_PROFILE")"
  fi
  if [[ -z "$RUSTPANEL_DISABLED_MODULES" ]]; then
    RUSTPANEL_DISABLED_MODULES="$(disabled_modules_for_profile "$RUSTPANEL_INSTALL_PROFILE")"
  fi
}

print_recommendations() {
  cat <<EOF
[RustPanel] Host detection
  Memory:        ${DETECTED_MEMORY_MB:-0} MB
  Disk free:     ${DETECTED_DISK_MB:-0} MB
  Virtualization:${DETECTED_VIRT:-unknown}
  Docker ready:  $(docker_ready && printf yes || printf no)

[RustPanel] Recommended install
  Profile:       $RUSTPANEL_INSTALL_PROFILE (detected: ${RECOMMENDED_PROFILE:-unknown})
  Mode:          $RUSTPANEL_INSTALL_MODE
  Enabled:       $RUSTPANEL_ENABLED_MODULES
  Disabled:      ${RUSTPANEL_DISABLED_MODULES:-none}
EOF
}

# 通用文本提问;空回车取默认值。回显默认值在方括号里。
# 提示语写到 stderr,这样命令替换 $(ask ...) 只会捕获用户答案,不会把 prompt 一起捕获进去。
ask() {
  local label="$1"
  local default="$2"
  local answer
  if [[ -n "$default" ]]; then
    printf '[RustPanel] %s [%s]: ' "$label" "$default" >&2
  else
    printf '[RustPanel] %s: ' "$label" >&2
  fi
  IFS= read -r answer || answer=""
  if [[ -z "$answer" ]]; then
    printf '%s' "$default"
  else
    printf '%s' "$answer"
  fi
}

# 默认 N 的 y/N 提问,返回 0 表示用户选 yes
ask_yesno() {
  local label="$1"
  local default="${2:-n}"
  local hint="[y/N]"
  if [[ "$default" == "y" ]]; then
    hint="[Y/n]"
  fi
  printf '[RustPanel] %s %s ' "$label" "$hint"
  local answer
  IFS= read -r answer || answer=""
  if [[ -z "$answer" ]]; then
    answer="$default"
  fi
  case "$answer" in
    y|Y|yes|YES) return 0 ;;
    *) return 1 ;;
  esac
}

# 密码提问:静默输入,空回车=随机生成。提示和换行都走 stderr,只让密码走 stdout 被命令替换捕获。
ask_password() {
  local label="$1"
  local answer
  printf '[RustPanel] %s (empty = generate random): ' "$label" >&2
  IFS= read -rs answer || answer=""
  printf '\n' >&2
  printf '%s' "$answer"
}

# 应用极限低内存模板:从 enabled 中移除 proxy/workloads,追加进 disabled 列表
apply_ultra_low_overrides() {
  if [[ "$RUSTPANEL_ULTRA_LOW" != "1" ]]; then
    return
  fi
  local module
  local kept=""
  local -a _modules=()
  IFS=',' read -r -a _modules <<< "$RUSTPANEL_ENABLED_MODULES"
  for module in "${_modules[@]}"; do
    case "$module" in
      proxy|workloads) ;;
      '') ;;
      *) kept+="${kept:+,}$module" ;;
    esac
  done
  RUSTPANEL_ENABLED_MODULES="$kept"
  case ",$RUSTPANEL_DISABLED_MODULES," in
    *,proxy,*) ;;
    *) RUSTPANEL_DISABLED_MODULES="${RUSTPANEL_DISABLED_MODULES:+$RUSTPANEL_DISABLED_MODULES,}proxy" ;;
  esac
  case ",$RUSTPANEL_DISABLED_MODULES," in
    *,workloads,*) ;;
    *) RUSTPANEL_DISABLED_MODULES="${RUSTPANEL_DISABLED_MODULES:+$RUSTPANEL_DISABLED_MODULES,}workloads" ;;
  esac
  log "Ultra-low template applied: proxy and workloads disabled"
}

# 默认交互配置:依次提问 NAT 范围 / 面板端口 / 公网地址 / 绑定 / Profile / Origin / 凭据 / 极限模板
# 非 TTY 或 --assume-recommended 时跳过,保留现有静默行为。
interactive_configure() {
  if [[ "$ASSUME_RECOMMENDED" == "1" || ! -t 0 ]]; then
    return
  fi

  printf '\n[RustPanel] Entering interactive setup. Press Enter to accept the [default] for each prompt.\n'
  printf '[RustPanel] Pass --assume-recommended next time to skip these prompts.\n\n'

  # NAT 端口范围 — 决定面板端口可选区间
  local nat_input
  nat_input="$(ask 'NAT-opened port range from your VPS provider, e.g. 1200-1219 (empty = no NAT restriction)' "$RUSTPANEL_NAT_PORT_RANGE")"
  parse_nat_port_range "$nat_input"
  RUSTPANEL_NAT_PORT_RANGE="$nat_input"

  # 面板访问端口 — 浏览器实际访问的端口,必须落在 NAT 范围内
  local default_port="$RUSTPANEL_API_PORT"
  if [[ -n "$NAT_PORT_LOW" ]]; then
    if (( default_port < NAT_PORT_LOW || default_port > NAT_PORT_HIGH )); then
      default_port="$NAT_PORT_LOW"
    fi
  fi
  while :; do
    RUSTPANEL_API_PORT="$(ask 'Panel access port (the port you type in browser, http://<public-ip>:<port>)' "$default_port")"
    if validate_port_against_nat "$RUSTPANEL_API_PORT"; then
      break
    fi
    log "please pick another port"
  done

  # 绑定地址
  RUSTPANEL_BIND_HOST="$(ask 'Bind address (keep 0.0.0.0 unless you put a reverse proxy in front)' "$RUSTPANEL_BIND_HOST")"

  # 公网地址 — NAT 机上 hostname -I 拿的是内网,必须让用户覆盖
  local detected_host
  detected_host="$(public_ip)"
  local default_public="$RUSTPANEL_PUBLIC_HOST"
  if [[ -z "$default_public" ]]; then
    default_public="$detected_host"
  fi
  if [[ -n "$detected_host" && "$detected_host" =~ ^(10\.|192\.168\.|172\.(1[6-9]|2[0-9]|3[0-1])\.|127\.) ]]; then
    log "Detected address $detected_host looks INTERNAL — replace it with the public IP/hostname your VPS provider gave you, otherwise the panel URL printed at the end will not be reachable."
  fi
  RUSTPANEL_PUBLIC_HOST="$(ask 'Public IP or hostname users will type in browser' "$default_public")"

  # Profile
  while :; do
    RUSTPANEL_INSTALL_PROFILE="$(ask 'Install profile (auto/micro/lite/standard/full)' "$RUSTPANEL_INSTALL_PROFILE")"
    case "$RUSTPANEL_INSTALL_PROFILE" in
      auto|micro|lite|standard|full) break ;;
      *) log "profile must be auto, micro, lite, standard, or full" ;;
    esac
  done
  # Profile 改了之后,重算模式与默认模块清单
  if [[ "$RUSTPANEL_INSTALL_MODE" == "auto" ]]; then
    RUSTPANEL_INSTALL_MODE="$(install_mode_for_profile "$RUSTPANEL_INSTALL_PROFILE")"
  fi
  RUSTPANEL_ENABLED_MODULES="$(modules_for_profile "$RUSTPANEL_INSTALL_PROFILE")"
  RUSTPANEL_DISABLED_MODULES="$(disabled_modules_for_profile "$RUSTPANEL_INSTALL_PROFILE")"

  # HTTPS Origin
  RUSTPANEL_ALLOWED_ORIGINS="$(ask 'Allowed browser origin for HTTPS reverse proxy (empty = none)' "$RUSTPANEL_ALLOWED_ORIGINS")"

  # 管理员凭据
  RUSTPANEL_ADMIN_USERNAME="$(ask 'Admin username' "$RUSTPANEL_ADMIN_USERNAME")"
  if [[ -z "$RUSTPANEL_ADMIN_PASSWORD" ]]; then
    RUSTPANEL_ADMIN_PASSWORD="$(ask_password 'Admin password')"
  fi

  # 极限低内存模板 — 仅当 micro profile 时问
  if [[ "$RUSTPANEL_INSTALL_PROFILE" == "micro" && "$RUSTPANEL_ULTRA_LOW" != "1" ]]; then
    if ask_yesno 'Disable proxy and workloads modules to save RAM on <=128MB hosts?' "n"; then
      RUSTPANEL_ULTRA_LOW=1
    fi
  fi

  # 最终预览 + 确认
  apply_ultra_low_overrides
  printf '\n[RustPanel] Final configuration:\n'
  printf '  Profile:       %s\n' "$RUSTPANEL_INSTALL_PROFILE"
  printf '  Mode:          %s\n' "$RUSTPANEL_INSTALL_MODE"
  printf '  Enabled:       %s\n' "$RUSTPANEL_ENABLED_MODULES"
  printf '  Disabled:      %s\n' "${RUSTPANEL_DISABLED_MODULES:-none}"
  printf '  NAT range:     %s\n' "${RUSTPANEL_NAT_PORT_RANGE:-none}"
  printf '  Panel URL:     http://%s:%s\n' "${RUSTPANEL_PUBLIC_HOST:-<your-host>}" "$RUSTPANEL_API_PORT"
  printf '  Admin user:    %s\n' "$RUSTPANEL_ADMIN_USERNAME"
  printf '\n'
  if ! ask_yesno 'Proceed with installation?' "y"; then
    fail "installation cancelled"
  fi
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
    --public-host)
      RUSTPANEL_PUBLIC_HOST="${2:?missing value for --public-host}"
      shift 2
      ;;
    --nat-port-range)
      RUSTPANEL_NAT_PORT_RANGE="${2:?missing value for --nat-port-range}"
      shift 2
      ;;
    --ultra-low)
      RUSTPANEL_ULTRA_LOW=1
      shift
      ;;
    --image)
      BACKEND_IMAGE="${2:?missing value for --image}"
      shift 2
      ;;
    --binary-url)
      BINARY_URL="${2:?missing value for --binary-url}"
      shift 2
      ;;
    --profile)
      RUSTPANEL_INSTALL_PROFILE="${2:?missing value for --profile}"
      shift 2
      ;;
    --install-mode)
      RUSTPANEL_INSTALL_MODE="${2:?missing value for --install-mode}"
      shift 2
      ;;
    --modules)
      RUSTPANEL_ENABLED_MODULES="${2:?missing value for --modules}"
      shift 2
      ;;
    --disable-modules)
      RUSTPANEL_DISABLED_MODULES="${2:?missing value for --disable-modules}"
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
    --assume-recommended)
      ASSUME_RECOMMENDED=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
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

case "$RUSTPANEL_API_PORT" in
  ''|*[!0-9]*)
    fail "--port must be a number"
    ;;
esac

if (( RUSTPANEL_API_PORT < 1 || RUSTPANEL_API_PORT > 65535 )); then
  fail "--port must be between 1 and 65535"
fi

case "$RUSTPANEL_INSTALL_PROFILE" in
  auto|micro|lite|standard|full) ;;
  *) fail "--profile must be auto, micro, lite, standard, or full" ;;
esac

case "$RUSTPANEL_INSTALL_MODE" in
  auto|docker|binary) ;;
  *) fail "--install-mode must be auto, docker, or binary" ;;
esac

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
  BINARY_URL="${RUSTPANEL_BINARY_URL:-$BINARY_URL}"
  RUSTPANEL_INSTALL_PROFILE="${RUSTPANEL_INSTALL_PROFILE:-auto}"
  RUSTPANEL_INSTALL_MODE="${RUSTPANEL_INSTALL_MODE:-auto}"
  RUSTPANEL_ENABLED_MODULES="${RUSTPANEL_ENABLED_MODULES:-}"
  RUSTPANEL_DISABLED_MODULES="${RUSTPANEL_DISABLED_MODULES:-}"
  RUSTPANEL_NAT_PORT_RANGE="${RUSTPANEL_NAT_PORT_RANGE:-}"
  RUSTPANEL_PUBLIC_HOST="${RUSTPANEL_PUBLIC_HOST:-}"
  RUSTPANEL_ULTRA_LOW="${RUSTPANEL_ULTRA_LOW:-0}"
  COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-rustpanel}"
  GITHUB_TOKEN="${GITHUB_TOKEN:-}"
  GHCR_USERNAME="${GHCR_USERNAME:-_}"
fi

# 用 CLI 或 env 传入的 NAT 范围预先解析,供后续校验和交互默认值使用
parse_nat_port_range "$RUSTPANEL_NAT_PORT_RANGE"

preflight_resources
apply_recommendations
print_recommendations
if [[ "$DRY_RUN" == "1" ]]; then
  exit 0
fi
interactive_configure
apply_ultra_low_overrides

# 交互或 CLI 调整后再次校验关键值
case "$RUSTPANEL_API_PORT" in
  ''|*[!0-9]*)
    fail "panel port must be a number"
    ;;
esac
if (( RUSTPANEL_API_PORT < 1 || RUSTPANEL_API_PORT > 65535 )); then
  fail "panel port must be between 1 and 65535"
fi
if [[ -n "$NAT_PORT_LOW" ]]; then
  if (( RUSTPANEL_API_PORT < NAT_PORT_LOW || RUSTPANEL_API_PORT > NAT_PORT_HIGH )); then
    fail "panel port $RUSTPANEL_API_PORT is outside NAT range $NAT_PORT_LOW-$NAT_PORT_HIGH"
  fi
fi
case "$RUSTPANEL_INSTALL_PROFILE" in
  auto|micro|lite|standard|full) ;;
  *) fail "profile must be auto, micro, lite, standard, or full" ;;
esac
case "$RUSTPANEL_INSTALL_MODE" in
  auto|docker|binary) ;;
  *) fail "install mode must be auto, docker, or binary" ;;
esac

if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  fail "please run as root, for example: sudo bash install.sh"
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

download_backend_binary() {
  local bin_dir="$INSTALL_DIR/bin"
  local archive="/tmp/rustpanel-backend.tar.gz"
  install -d -m 0755 "$bin_dir"
  log "Downloading RustPanel backend binary"
  download_file "$BINARY_URL" "$archive"
  tar -xzf "$archive" -C "$bin_dir"
  if [[ ! -x "$bin_dir/rustpanel-backend" ]]; then
    local found
    found="$(find "$bin_dir" -type f -name rustpanel-backend -perm -111 | head -n 1)"
    [[ -n "$found" ]] || fail "rustpanel-backend binary not found in $BINARY_URL"
    if [[ "$found" != "$bin_dir/rustpanel-backend" ]]; then
      cp "$found" "$bin_dir/rustpanel-backend"
    fi
  fi
  chmod +x "$bin_dir/rustpanel-backend"
}

write_systemd_service() {
  local service_path="/etc/systemd/system/rustpanel-backend.service"
  cat > "$service_path" <<EOF
[Unit]
Description=RustPanel backend service
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=$INSTALL_DIR
EnvironmentFile=$INSTALL_DIR/.env
ExecStart=$INSTALL_DIR/bin/rustpanel-backend
Restart=always
RestartSec=3
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF
  systemctl daemon-reload
  systemctl enable --now rustpanel-backend
}

start_binary_backend() {
  if command -v systemctl >/dev/null 2>&1; then
    write_systemd_service
    return
  fi

  log "systemd not found, starting RustPanel with daemon mode"
  set -a
  # shellcheck disable=SC1091
  source "$INSTALL_DIR/.env"
  set +a
  "$INSTALL_DIR/bin/rustpanel-backend" --daemon
}

install_proxy_runtime() {
  case ",$RUSTPANEL_ENABLED_MODULES," in
    *,proxy,*) ;;
    *) return ;;
  esac
  local bin_dir="$INSTALL_DIR/bin"
  local target="$bin_dir/ssserver"
  if [[ -x "$target" ]]; then
    return
  fi
  if [[ -n "${RUSTPANEL_SHADOWSOCKS_SERVER_URL:-}" ]]; then
    install_shadowsocks_from_url "$RUSTPANEL_SHADOWSOCKS_SERVER_URL" "$target" || true
    return
  fi
  if command -v curl >/dev/null 2>&1; then
    local api asset
    api="$(curl -fsSL https://api.github.com/repos/shadowsocks/shadowsocks-rust/releases/latest 2>/dev/null || true)"
    asset="$(printf '%s\n' "$api" | sed -n 's/.*"browser_download_url": "\(.*x86_64-unknown-linux-gnu.*\.tar\.xz\)".*/\1/p' | head -n 1)"
    if [[ -n "$asset" ]]; then
      install_shadowsocks_from_url "$asset" "$target" || true
    fi
  fi
  if [[ ! -x "$target" ]]; then
    log "shadowsocks-rust runtime was not installed; set RUSTPANEL_SHADOWSOCKS_SERVER_URL and rerun if proxy start needs ssserver"
  fi
}

install_shadowsocks_from_url() {
  local url="$1"
  local target="$2"
  local archive="/tmp/rustpanel-shadowsocks.tar.xz"
  local extract_dir="/tmp/rustpanel-shadowsocks"
  download_file "$url" "$archive"
  rm -rf "$extract_dir"
  install -d "$extract_dir"
  tar -xJf "$archive" -C "$extract_dir"
  local found
  found="$(find "$extract_dir" -type f -name ssserver -perm -111 | head -n 1)"
  [[ -n "$found" ]] || return 1
  install -m 0755 "$found" "$target"
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
  write_env_var "RUSTPANEL_BINARY_URL" "$BINARY_URL"
  write_env_var "RUSTPANEL_INSTALL_DIR" "$INSTALL_DIR"
  write_env_var "RUSTPANEL_DATA_DIR" "$DATA_DIR"
  write_env_var "RUSTPANEL_INSTALL_PROFILE" "$RUSTPANEL_INSTALL_PROFILE"
  write_env_var "RUSTPANEL_INSTALL_MODE" "$RUSTPANEL_INSTALL_MODE"
  write_env_var "RUSTPANEL_ENABLED_MODULES" "$RUSTPANEL_ENABLED_MODULES"
  write_env_var "RUSTPANEL_DISABLED_MODULES" "$RUSTPANEL_DISABLED_MODULES"
  write_env_var "RUSTPANEL_NAT_PORT_RANGE" "$RUSTPANEL_NAT_PORT_RANGE"
  write_env_var "RUSTPANEL_PUBLIC_HOST" "$RUSTPANEL_PUBLIC_HOST"
  write_env_var "RUSTPANEL_ULTRA_LOW" "$RUSTPANEL_ULTRA_LOW"
  write_env_var "RUSTPANEL_BACKEND_ADDR" "$RUSTPANEL_BIND_HOST:$RUSTPANEL_API_PORT"
  write_env_var "RUSTPANEL_BIND_HOST" "$RUSTPANEL_BIND_HOST"
  write_env_var "RUSTPANEL_API_PORT" "$RUSTPANEL_API_PORT"
  write_env_var "RUSTPANEL_ALLOWED_ORIGINS" "$RUSTPANEL_ALLOWED_ORIGINS"
  write_env_var "RUSTPANEL_ADMIN_USERNAME" "$RUSTPANEL_ADMIN_USERNAME"
  write_env_var "RUSTPANEL_ADMIN_PASSWORD" "$RUSTPANEL_ADMIN_PASSWORD"
  write_env_var "RUSTPANEL_JWT_SECRET" "$RUSTPANEL_JWT_SECRET"
  write_env_var "RUSTPANEL_AUDIT_ROOT" "$DATA_DIR/audit"
  write_env_var "RUSTPANEL_CLUSTER_ROOT" "$DATA_DIR/cluster"
  write_env_var "RUSTPANEL_CRON_ROOT" "$DATA_DIR/cron"
  write_env_var "RUSTPANEL_FILE_STATE_ROOT" "$DATA_DIR/files"
  write_env_var "RUSTPANEL_SECURITY_ROOT" "$DATA_DIR/security"
  write_env_var "RUSTPANEL_SITE_STATE_ROOT" "$DATA_DIR/site"
  write_env_var "RUSTPANEL_WORKLOAD_ROOT" "$DATA_DIR/workloads"
  write_env_var "RUSTPANEL_PROXY_ROOT" "$DATA_DIR/proxy"
  write_env_var "RUSTPANEL_PROXY_BIN_DIR" "$INSTALL_DIR/bin"
  if [[ "$RUSTPANEL_INSTALL_PROFILE" == "micro" ]]; then
    write_env_var "RUSTPANEL_SITE_ENGINE" "builtin"
    write_env_var "RUSTPANEL_SECURITY_APPLY" "0"
  else
    write_env_var "RUSTPANEL_SITE_ENGINE" "nginx"
  fi
  write_env_var "GITHUB_TOKEN" "$GITHUB_TOKEN"
  write_env_var "GHCR_USERNAME" "$GHCR_USERNAME"
} > "$INSTALL_DIR/.env"

if [[ "$RUSTPANEL_INSTALL_MODE" == "binary" ]]; then
  download_backend_binary
  install_proxy_runtime
  log "Starting RustPanel in binary mode"
  start_binary_backend
else
  install_docker
  ensure_compose_plugin
  start_docker

  log "Starting RustPanel in Docker mode"
  bash "$INSTALL_DIR/deploy/update.sh"
fi

# 优先使用用户在交互或 CLI 中给出的公网地址,fallback 到自动探测;localhost 绑定时退回绑定地址
host="${RUSTPANEL_PUBLIC_HOST:-$(public_ip)}"
if [[ -z "$host" || "$RUSTPANEL_BIND_HOST" == "127.0.0.1" || "$RUSTPANEL_BIND_HOST" == "localhost" ]]; then
  host="$RUSTPANEL_BIND_HOST"
fi

extra_notes=""
if [[ -n "$RUSTPANEL_NAT_PORT_RANGE" ]]; then
  extra_notes+="
NAT range:   $RUSTPANEL_NAT_PORT_RANGE (panel port $RUSTPANEL_API_PORT must remain inside this range)"
fi
if [[ "$RUSTPANEL_ULTRA_LOW" == "1" ]]; then
  extra_notes+="
Ultra-low:   proxy and workloads modules disabled to save RAM"
fi

cat <<EOF

RustPanel installed successfully.

Panel URL: http://$host:$RUSTPANEL_API_PORT
Username:  $RUSTPANEL_ADMIN_USERNAME
Password:  $RUSTPANEL_ADMIN_PASSWORD

Install dir: $INSTALL_DIR
Data dir:    $DATA_DIR
Update:      bash $INSTALL_DIR/deploy/update.sh$extra_notes

Keep $INSTALL_DIR/.env private. It contains the admin password and JWT secret.
EOF
