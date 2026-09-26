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

# 无 systemd 时的证书续签后备:cron.d 每天一次。
rustpanel_write_cert_renew_cron() {
  [[ -d "$RUSTPANEL_CRON_D_DIR" ]] || return 0
  cat > "$RUSTPANEL_CRON_D_DIR/rustpanel-cert-renew" <<EOF
# 由 RustPanel 安装器生成:每天续签一次临期证书
SHELL=/bin/sh
17 3 * * * root set -a; . '$INSTALL_DIR/.env'; set +a; MALLOC_ARENA_MAX=2 TOKIO_WORKER_THREADS=1 '$INSTALL_DIR/bin/rustpanel-backend' --renew-certs >/dev/null 2>&1
EOF
  chmod 0644 "$RUSTPANEL_CRON_D_DIR/rustpanel-cert-renew"
}

# 写全部单元并按节俭模式切换启动方式。已在跑的面板会被重启。
rustpanel_write_units() {
  local frugal="${RUSTPANEL_FRUGAL:-0}"
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
  systemctl daemon-reload
  if [[ "$frugal" == "1" ]]; then
    systemctl enable --now rustpanel-backend.socket
  fi
  systemctl enable --now rustpanel-backend.service
  systemctl enable --now rustpanel-cert-renew.timer
}
