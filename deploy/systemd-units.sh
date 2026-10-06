#!/usr/bin/env bash
# RustPanel 二进制模式的 systemd / cron 单元生成,install.sh 与 update.sh 共用。
#
# source 本文件后调用 rustpanel_write_units,依赖变量:
#   INSTALL_DIR  RUSTPANEL_BIND_HOST  RUSTPANEL_API_PORT  RUSTPANEL_FRUGAL(1/0)
#
# 节俭模式(RUSTPANEL_FRUGAL=1,micro 档默认):监听端口交给
# rustpanel-backend.socket,面板空闲 N 分钟(RUSTPANEL_IDLE_EXIT_MINUTES,默认 10)
# 后 exit 0,下一个连接再由 systemd 拉起。证书续签走 rustpanel-cert-renew.timer
# 调一次性命令,计划任务由面板写进 /etc/cron.d/rustpanel,都不依赖面板常驻。

RUSTPANEL_SYSTEMD_DIR="${RUSTPANEL_SYSTEMD_DIR:-/etc/systemd/system}"
RUSTPANEL_CRON_D_DIR="${RUSTPANEL_CRON_D_DIR:-/etc/cron.d}"
RUSTPANEL_OPENRC_INIT_DIR="${RUSTPANEL_OPENRC_INIT_DIR:-/etc/init.d}"
RUSTPANEL_OPENRC_LOG_DIR="${RUSTPANEL_OPENRC_LOG_DIR:-/var/log/rustpanel}"

# systemd 是否真的在当 PID 1——跟 Rust 那边 service_manager::detect() 判断标准
# 一致,不是看有没有 systemctl 命令(容器里经常有命令没有真正的 systemd)。
rustpanel_has_systemd() {
  [[ -d /run/systemd/system ]]
}

rustpanel_has_openrc() {
  command -v rc-service >/dev/null 2>&1 || command -v openrc >/dev/null 2>&1
}

rustpanel_listen_stream() {
  local host="$1"
  local port="$2"
  case "$host" in
    *:*)
      host="${host#[}"
      printf '[%s]:%s' "${host%]}" "$port"
      ;;
    *) printf '%s:%s' "$host" "$port" ;;
  esac
}

# 把 KEY=VALUE 补进 .env(已存在同名 key 时不动),用于老安装升级时补新配置。
rustpanel_ensure_env_var() {
  local env_file="$1"
  local key="$2"
  local value="$3"
  [[ -f "$env_file" ]] || return 0
  if ! grep -q "^${key}=" "$env_file"; then
    # 手改过的 .env 可能没有结尾换行,直接追加会和最后一行粘在一起
    if [[ -s "$env_file" && -n "$(tail -c 1 "$env_file")" ]]; then
      printf '\n' >> "$env_file"
    fi
    printf "%s='%s'\n" "$key" "$value" >> "$env_file"
  fi
}

rustpanel_write_backend_service() {
  local frugal="$1"
  local restart="always"
  local socket_deps=""
  if [[ "$frugal" == "1" ]]; then
    # 空闲退出是 exit 0:只在异常时重启,正常退出后等 socket 按需唤醒
    restart="on-failure"
    socket_deps="Requires=rustpanel-backend.socket
After=rustpanel-backend.socket"
  fi
  cat > "$RUSTPANEL_SYSTEMD_DIR/rustpanel-backend.service" <<EOF
[Unit]
Description=RustPanel backend service
After=network-online.target
Wants=network-online.target
$socket_deps

[Service]
Type=simple
WorkingDirectory=$INSTALL_DIR
EnvironmentFile=$INSTALL_DIR/.env
# glibc 默认每核 8 个 malloc arena,多线程下 RSS 虚胖;2 个足够面板用
Environment=MALLOC_ARENA_MAX=2
ExecStart=$INSTALL_DIR/bin/rustpanel-backend
Restart=$restart
RestartSec=3
# 网页终端里的交互式 shell 会忽略 SIGTERM,默认要等满 90s 才强杀;
# 15s 足够托管的 workload / proxy 正常退出
TimeoutStopSec=15
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF
}

rustpanel_write_backend_socket() {
  cat > "$RUSTPANEL_SYSTEMD_DIR/rustpanel-backend.socket" <<EOF
[Unit]
Description=RustPanel backend socket (frugal mode, starts the panel on demand)

[Socket]
ListenStream=$(rustpanel_listen_stream "$RUSTPANEL_BIND_HOST" "$RUSTPANEL_API_PORT")
NoDelay=true

[Install]
WantedBy=sockets.target
EOF
}

rustpanel_write_cert_renew_timer() {
  cat > "$RUSTPANEL_SYSTEMD_DIR/rustpanel-cert-renew.service" <<EOF
[Unit]
Description=RustPanel certificate renewal (one-shot)
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
WorkingDirectory=$INSTALL_DIR
EnvironmentFile=$INSTALL_DIR/.env
Environment=MALLOC_ARENA_MAX=2
Environment=TOKIO_WORKER_THREADS=1
ExecStart=$INSTALL_DIR/bin/rustpanel-backend --renew-certs
NoNewPrivileges=true
EOF
  cat > "$RUSTPANEL_SYSTEMD_DIR/rustpanel-cert-renew.timer" <<EOF
[Unit]
Description=Daily RustPanel certificate renewal

[Timer]
OnCalendar=*-*-* 03:17:00
RandomizedDelaySec=1h
Persistent=true

[Install]
WantedBy=timers.target
EOF
}

# 节俭模式下面板大部分时间在休眠,进程内的告警扫描器跟着停了;
# 改由这个 timer 每 15 分钟拉起一次性的 --scan-alerts(没配通知渠道时秒退)。
rustpanel_write_alerts_timer() {
  cat > "$RUSTPANEL_SYSTEMD_DIR/rustpanel-alerts.service" <<EOF
[Unit]
Description=RustPanel alert scan (one-shot, frugal mode)
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
WorkingDirectory=$INSTALL_DIR
EnvironmentFile=$INSTALL_DIR/.env
Environment=MALLOC_ARENA_MAX=2
Environment=TOKIO_WORKER_THREADS=1
ExecStart=$INSTALL_DIR/bin/rustpanel-backend --scan-alerts
NoNewPrivileges=true
EOF
  cat > "$RUSTPANEL_SYSTEMD_DIR/rustpanel-alerts.timer" <<EOF
[Unit]
Description=RustPanel alert scan every 15 minutes

[Timer]
OnCalendar=*:0/15
RandomizedDelaySec=60

[Install]
WantedBy=timers.target
EOF
}

# OpenRC 版本的常驻服务:没有 .socket unit 这种概念,节俭模式直接把
# `rustpanel-backend --activate`(可移植的 socket activation 监督者,见
# src/backend/src/activate.rs)交给 supervise-daemon 托管——监督者自己
# bind 端口、按需 fork 真正的服务进程,OpenRC 只需要管这一个进程。
# 非节俭模式就直接管面板二进制本身,效果上对应 systemd 的 Restart=always。
rustpanel_write_openrc_service() {
  local frugal="$1"
  local listen command_args
  listen="$(rustpanel_listen_stream "$RUSTPANEL_BIND_HOST" "$RUSTPANEL_API_PORT")"
  if [[ "$frugal" == "1" ]]; then
    command_args="--activate --addr $listen --bin $INSTALL_DIR/bin/rustpanel-backend"
  else
    command_args="--addr $listen"
  fi
  mkdir -p "$RUSTPANEL_OPENRC_LOG_DIR"
  cat > "$RUSTPANEL_OPENRC_INIT_DIR/rustpanel-backend" <<EOF
#!/sbin/openrc-run

description="RustPanel backend service"
command="$INSTALL_DIR/bin/rustpanel-backend"
command_args="$command_args"
directory="$INSTALL_DIR"
supervisor="supervise-daemon"
pidfile="/run/\${RC_SVCNAME}.pid"
respawn_delay=3
output_log="$RUSTPANEL_OPENRC_LOG_DIR/rustpanel-backend.log"
error_log="$RUSTPANEL_OPENRC_LOG_DIR/rustpanel-backend.log"

start_pre() {
	if [ -f "$INSTALL_DIR/.env" ]; then
		set -a
		. "$INSTALL_DIR/.env"
		set +a
	fi
	export MALLOC_ARENA_MAX=2
}

depend() {
	need net
	after net-online
}
EOF
  chmod 0755 "$RUSTPANEL_OPENRC_INIT_DIR/rustpanel-backend"
}

# 无 systemd 的 OpenRC 主机:rc-update/rc-service 替代 systemctl,证书续签和
# 节俭模式下的告警扫描都走 cron(rustpanel_cron_set 自动识别 cron.d 还是
# crontab 回退),OpenRC 没有 .timer 这种概念。
rustpanel_write_openrc_units() {
  local frugal="${RUSTPANEL_FRUGAL:-0}"
  rc-service rustpanel-backend stop >/dev/null 2>&1 || true
  rustpanel_write_openrc_service "$frugal"
  rc-update add rustpanel-backend default >/dev/null 2>&1 || true
  rc-service rustpanel-backend start
  rustpanel_write_cert_renew_cron
  if [[ "$frugal" == "1" ]]; then
    rustpanel_cron_set rustpanel-alerts "*/15 * * * *" \
      "set -a; . '$INSTALL_DIR/.env'; set +a; MALLOC_ARENA_MAX=2 TOKIO_WORKER_THREADS=1 '$INSTALL_DIR/bin/rustpanel-backend' --scan-alerts >/dev/null 2>&1"
  else
    rustpanel_cron_unset rustpanel-alerts
  fi
}

# 写一条系统级计划任务:优先用 /etc/cron.d,没有这个目录就退到 root 的 crontab
# (兼容 Alpine 默认的 busybox crond,它不读 /etc/cron.d)。用标记行包住 crontab 里
# 的那一段,方便覆盖/删除,不碰用户 crontab 里其它手写的内容。
# 用法:rustpanel_cron_set <name> <schedule 例如 "17 3 * * *"> <command>
rustpanel_cron_set() {
  local name="$1" schedule="$2" command="$3"
  if [[ -d "$RUSTPANEL_CRON_D_DIR" ]]; then
    cat > "$RUSTPANEL_CRON_D_DIR/$name" <<EOF
# 由 RustPanel 安装器生成,勿手动编辑
SHELL=/bin/sh
$schedule root $command
EOF
    chmod 0644 "$RUSTPANEL_CRON_D_DIR/$name"
    return 0
  fi
  # 自定义 cron.d 目录用于隔离测试;目录不存在时不能回退污染宿主 crontab。
  # 正式回退只适用于 Linux root(例如 Alpine),不能写开发机用户的 crontab。
  if [[ "$RUSTPANEL_CRON_D_DIR" != /etc/cron.d ]] ||
     [[ "$(uname -s)" != Linux || "$(id -u)" != 0 ]]; then
    printf '%s\n' 'RustPanel: refusing host crontab fallback outside Linux root with the default cron directory' >&2
    return 1
  fi
  if command -v crontab >/dev/null 2>&1; then
    local begin="# rustpanel:$name begin" end="# rustpanel:$name end" existing
    existing="$(crontab -l 2>/dev/null || true)"
    {
      printf '%s\n' "$existing" | awk -v b="$begin" -v e="$end" '
        $0==b { skip=1 }
        !skip { print }
        $0==e { skip=0 }
      '
      printf '%s\n%s %s\n%s\n' "$begin" "$schedule" "$command" "$end"
    } | crontab -
    return 0
  fi
  return 1
}

rustpanel_cron_unset() {
  local name="$1"
  if [[ -d "$RUSTPANEL_CRON_D_DIR" ]]; then
    rm -f "$RUSTPANEL_CRON_D_DIR/$name"
  fi
  # 隔离目录的清理同样不得访问宿主 crontab。
  [[ "$RUSTPANEL_CRON_D_DIR" == /etc/cron.d ]] || return 0
  [[ "$(uname -s)" == Linux && "$(id -u)" == 0 ]] || return 0
  if command -v crontab >/dev/null 2>&1; then
    local begin="# rustpanel:$name begin" end="# rustpanel:$name end" existing
    existing="$(crontab -l 2>/dev/null || true)"
    [[ -n "$existing" ]] || return 0
    printf '%s\n' "$existing" | awk -v b="$begin" -v e="$end" '
      $0==b { skip=1; next }
      $0==e { skip=0; next }
      !skip { print }
    ' | crontab -
  fi
}

# 无 systemd 时的证书续签后备:每天一次。
rustpanel_write_cert_renew_cron() {
  rustpanel_cron_set rustpanel-cert-renew "17 3 * * *" \
    "set -a; . '$INSTALL_DIR/.env'; set +a; MALLOC_ARENA_MAX=2 TOKIO_WORKER_THREADS=1 '$INSTALL_DIR/bin/rustpanel-backend' --renew-certs >/dev/null 2>&1"
}

# 小硬盘优化(micro 档默认):apt 不留 .deb / pkgcache、索引保持压缩;journal 上限 20MB
# (默认是分区的 10%,2GB 盘就是 ~190MB)。都是可重建的缓存,不影响功能,只是 apt 略慢。
rustpanel_apply_small_disk_tweaks() {
  if [[ -d /etc/apt/apt.conf.d ]]; then
    cat > /etc/apt/apt.conf.d/99rustpanel-small-disk <<'APTCONF'
// RustPanel 小硬盘优化:不留 .deb 包、不落 pkgcache 二进制缓存、索引保持压缩
Dir::Cache::pkgcache "";
Dir::Cache::srcpkgcache "";
Acquire::GzipIndexes "true";
Acquire::CompressionTypes::Order:: "gz";
DPkg::Post-Invoke { "rm -f /var/cache/apt/archives/*.deb /var/cache/apt/archives/partial/*.deb /var/cache/apt/*.bin || true"; };
APT::Update::Post-Invoke { "rm -f /var/cache/apt/archives/*.deb /var/cache/apt/archives/partial/*.deb /var/cache/apt/*.bin || true"; };
APTCONF
    apt-get clean >/dev/null 2>&1 || true
    rm -f /var/cache/apt/*.bin
  fi
  if [[ -d /etc/systemd ]]; then
    mkdir -p /etc/systemd/journald.conf.d
    printf '[Journal]\nSystemMaxUse=20M\n' > /etc/systemd/journald.conf.d/rustpanel-small-disk.conf
    systemctl restart systemd-journald >/dev/null 2>&1 || true
  fi
}

# 写全部单元并按节俭模式切换启动方式。已在跑的面板会被重启。
rustpanel_write_units() {
  local frugal="${RUSTPANEL_FRUGAL:-0}"
  # 老版 update.sh 会把新发布包整个解压进 bin/:把包里的新 update.sh 就位并清掉 bin/deploy,
  # 之后的升级就走新流程(包内脚本、原子替换、自更新)
  if [[ -f "$INSTALL_DIR/bin/deploy/update.sh" ]]; then
    install -m 0755 "$INSTALL_DIR/bin/deploy/update.sh" "$INSTALL_DIR/deploy/update.sh.new"
    mv -f "$INSTALL_DIR/deploy/update.sh.new" "$INSTALL_DIR/deploy/update.sh"
    rm -rf "$INSTALL_DIR/bin/deploy"
  fi
  if ! rustpanel_has_systemd; then
    rustpanel_write_openrc_units
    return
  fi
  # 先停:非节俭 → 节俭时面板自己占着端口,socket 单元会 bind 失败
  systemctl stop rustpanel-backend.service >/dev/null 2>&1 || true
  rustpanel_write_backend_service "$frugal"
  if [[ "$frugal" == "1" ]]; then
    rustpanel_write_backend_socket
  else
    systemctl disable --now rustpanel-backend.socket >/dev/null 2>&1 || true
    rm -f "$RUSTPANEL_SYSTEMD_DIR/rustpanel-backend.socket"
  fi
  rustpanel_write_cert_renew_timer
  if [[ "$frugal" == "1" ]]; then
    rustpanel_write_alerts_timer
  else
    systemctl disable --now rustpanel-alerts.timer >/dev/null 2>&1 || true
    rm -f "$RUSTPANEL_SYSTEMD_DIR/rustpanel-alerts.service" "$RUSTPANEL_SYSTEMD_DIR/rustpanel-alerts.timer"
  fi
  systemctl daemon-reload
  if [[ "$frugal" == "1" ]]; then
    systemctl enable --now rustpanel-backend.socket
  fi
  systemctl enable --now rustpanel-backend.service
  systemctl enable --now rustpanel-cert-renew.timer
  if [[ "$frugal" == "1" ]]; then
    systemctl enable --now rustpanel-alerts.timer
  fi
}
