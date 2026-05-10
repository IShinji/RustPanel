# NAT VPS 部署指南

本文针对**128MB RAM / 2GB 磁盘 / NAT IPv4 + 20 端口 / OpenVZ /
NO SUPPORT** 这一类极小 VPS,给出 RustPanel 推荐配置与典型用法。
普通 VPS(独立 IP、≥1GB RAM)请直接走默认 `standard` profile,
不需要本文。

## 一、机器形状决定了能跑什么

| 资源 | 实际含义 |
|------|---------|
| 128MB RAM | RustPanel 自身 ~50MB,**业务可用 ~30–40MB**;无法跑 Docker / JVM / Postgres / NextCloud |
| 2GB 磁盘 | 系统 + RustPanel ~600MB,**业务可用 ~1.2–1.4GB**;不能本地构建镜像 |
| 125GB/月 | 静态站 / Bot / 备份客户端绰绰有余;视频中转、代理出口会爆 |
| NAT IPv4 + 20 端口 | **没有 80 / 443**;Cloudflare Tunnel 是最干净的入口方案 |
| /80 IPv6 | 2^48 地址,sites 模块的 IPv6-aware 反代直接挂 |
| OpenVZ | 无内核模块:WireGuard 只能 `wireguard-go` 用户态,Docker 走不通 |
| NO SUPPORT | 一旦自闭就报废,30 秒自动回滚护栏必须开 |

## 二、安装

```sh
# 安装脚本会自动检测 OpenVZ / 内存 / 磁盘,匹配 micro profile
curl -fsSL https://example.com/install.sh | bash -s -- \
  --profile micro \
  --ultra-low \
  --nat-port-range 1200-1219 \
  --public-host <对外 IPv4 或域名>
```

`--ultra-low` 会额外关掉 `workloads` / `proxy` 两个模块,把
RustPanel 自身占用压到 ~40MB。装完后 `appstore` / `sites` /
`ssl` / `security` 默认就是开的。

## 三、推荐应用组合(同时跑 3 件事是上限)

按用途任选其一:

### A. 个人门户型

```
RustPanel (micro + ultra-low)        ~50MB
Caddy 或 static-web-server           ~5–25MB
WireGuard (wireguard-go)              ~15MB
cron + restic 备份                   ~10MB 运行时
─────────────────────────────────────────────
合计                                  ~80MB
```

### B. 网络出口型

```
RustPanel                             ~50MB
leaf(多协议代理,默认 off,需手动启用) ~25MB
NSD / Unbound(权威或递归 DNS)         ~10MB
─────────────────────────────────────────────
合计                                  ~85MB
```

### C. 自动化中枢型

```
RustPanel                             ~50MB
Telegram bot (自写 Rust)               ~10MB
changedetection cron 脚本(自写)       ~10MB 运行时
Mosquitto MQTT broker                  ~5MB
─────────────────────────────────────────────
合计                                  ~75MB
```

## 四、Phase G Rust 栈(可选,在软件商店启用)

软件商店里有 5 个专为这类机器准备的 Rust 写的可选服务,
**全部走上游官方二进制,RustPanel 不 fork、不打补丁**:

| 包 | 用途 | 默认 RAM | 默认状态 |
|----|------|---------|---------|
| **rpxy** | HTTPS 反代 + 多站点 + ACME | 15MB | off,推荐 |
| **static-web-server** | 纯静态文件服务 | 5MB | off,推荐 |
| **leaf** | 多协议代理(SS/VLESS/Trojan/WG/h2/ws/tls) | 25MB | off |
| **vSMTP** | 邮件 alias 中转(出站走 SMTP relay) | 35MB | off |
| **TUIC v5** | UDP/QUIC 备用代理 | 20MB | off,实验性 |

安装即下载上游 release 的 musl 静态链接二进制 → 解压 → 安装
到 `/usr/local/bin/<slug>` → 写 systemd unit → `enable --now`。
卸载会停服务、删二进制,**但保留 config 与数据目录**,
重装可继续用之前的配置。

### 4.1 vSMTP 邮件中转的硬约束

- **出站必须走 SMTP relay**(Resend / SES / Postmark / Mailgun),
  绝不直连 25 端口 —— NAT VPS 99% 封 25 出站,自连必进对方垃圾箱
- **vSMTP 不是收件箱**:它只做收→改写→转发的过滤型 MTA,
  IMAP 收件请用 Gmail / ProtonMail

### 4.2 用 Cloudflare Email Routing 代替自建 vSMTP

只需要"自有域名能收信"且接受"回复出去会暴露 Gmail 地址"时,
**不要在 VPS 上跑 vSMTP**,直接用 Cloudflare Email Routing
(免费、零部署):

1. 域名托管在 Cloudflare
2. Email Routing → Routes,把 `*@yourdomain.com` 转给真实邮箱
3. SPF / DMARC 跟着 CF 走

只有需要"对方看到的回复地址是 `xxx@yourdomain.com` 而不是 Gmail
地址"这种 alias 双向链路时,才有必要装 vSMTP。

## 五、运维开关与逃生路径

### 5.1 软件商店执行干跑

CI / 开发机 / 容器内测试不该真的下载 / 装服务,设这两个 env
变量就可以让 RustPanel 走"只规划不执行"路径:

```sh
# BinaryDownload / NativePackage 路径:跳过下载 / apt / systemctl
RUSTPANEL_APPSTORE_SKIP_EXECUTE=1

# DockerCompose 路径:跳过实际 docker compose up/down
RUSTPANEL_APPSTORE_SKIP_COMPOSE=1

# systemd unit 写入目录(默认 /etc/systemd/system),
# 测试 / 容器内沙箱可指向 /tmp 子目录
RUSTPANEL_SYSTEMD_DIR=/tmp/systemd
```

### 5.2 NO SUPPORT 活命三件套(上线前必做)

1. **30 秒自动回滚护栏开起来**:任何改防火墙 / SSH / 面板入口
   的操作,都经 `ScheduleRollback` —— 30 秒内不主动 keepalive
   就自动还原,防止自己把自己锁外面。
2. **第二条进入路径**:除了 SSH,留一条 Cloudflare Tunnel /
   面板 web terminal 在另一个 NAT 端口,防火墙写歪时还有救。
3. **整机配置 cron 备份**:用 `restic`(appstore 已有)每天把
   `/etc` + RustPanel 数据目录推到 S3 / B2 / OneDrive,重装就靠它。

## 六、不要在这类机器上做的事

| 类别 | 软件 | 原因 |
|------|------|------|
| 个人云 | Nextcloud / Seafile / Immich | 256MB+ 起步 |
| Git 平台 | Gitea / Forgejo / GitLab | 150MB+ 起步 |
| 完整邮箱 | Mailcow / Mail-in-a-Box / Postfix+Dovecot+Rspamd | 256MB+ 且 25 端口出站基本封死 |
| 多模型 DB | SurrealDB / TiKV | 60MB+ 起步 |
| 游戏服 | Minecraft / Valheim | RAM 笑话 |
| 媒体 | Plex / Jellyfin / Frigate | RAM + 带宽都不够 |
| 容器化一切 | Docker / Podman | OpenVZ + 128MB 直接放弃 |

## 七、相关 commit 参考

- `b998aa0` —— Phase G 5 个 Rust 栈包模板入软件商店
- `ba74841` —— InstallPlan 数据契约 + Phase G versions + 描述里的官网
- `53f07da` —— `AppTemplate.homepage` proto 字段全数据化
- `f6ada66` —— BinaryDownload executor(curl + tar + systemctl)
- 本文档随后续 commit 落地
