# NAT VPS 部署指南

本文针对**128MB RAM / 2GB 磁盘 / NAT IPv4 + 20 端口 / OpenVZ /
NO SUPPORT** 这一类极小 VPS,给出 RustPanel 推荐配置与典型用法。
普通 VPS(独立 IP、≥1GB RAM)请直接走默认 `standard` profile,
不需要本文。

**磁盘比这还极限(128MB 级,不是 2GB)的小鸡见第七节**——同样是
"micro 档",但对机器形态的要求更窄,不是随便一台 128MB 内存的
VPS 都装得下。

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

1. **30 秒自动回滚护栏只覆盖面板入口三项设置,不覆盖防火墙 / SSH**:
   保存 `panel_listen_addr`(监听地址)、`panel_access_path`(访问路径)、
   `two_factor_required`(强制 2FA)任意一项时,`update_security_options`
   会自动调 `arm_rollback_watchdog` 经 `ScheduleRollback` 挂一个默认
   30 秒(最长可配到 600 秒)的倒计时:到点前没有主动确认,就用挂起
   时保存的 JSON 快照 + revert 命令自动还原这三项设置。**防火墙规则、
   SSH 端口 / 密钥、Cloudflare Access 等改动不触发这个护栏**,改这些
   之前自己按第 2 条留好退路。
   - 前端表现:改完上述任一项设置后,顶部会出现一条带实时倒计时的
     警告横幅(`RollbackBanner`,每 3 秒轮询一次待处理回滚),确认没问题
     就点横幅上的按钮调用 `ConfirmRollback` 拆除倒计时;不点,到点自动
     revert。
   - 状态落盘在 `/var/lib/rustpanel/security/state.json`
     (可用 `RUSTPANEL_SECURITY_ROOT` 覆盖根目录)。
   - **真被锁在面板入口外、30 秒又已经过期或者本来就不在这三项范围内**:
     SSH 进机器(或用服务商的 rescue / VNC 控制台),手动改回
     `state.json` 里的 `panel_listen_addr` / `panel_access_path` /
     `two_factor_required` 字段(或者从第 3 条的整机备份里把这个文件
     连同 `/etc` 一起恢复),然后 `systemctl restart rustpanel-backend.service`
     使其生效。
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

## 七、128MB 磁盘级(Alpine + LXC/OpenVZ 专属,不是本文前面那种 2GB 盘)

### 8.1 这个目标只在特定虚拟化类型上成立

**128MB 磁盘目标只在 LXC/OpenVZ 这类"共享宿主机内核、容器里不落盘内核
镜像"的虚拟化上现实。KVM 完整虚拟机不行**——KVM 上哪怕是 Alpine,自己
也要装一份内核 + initramfs + 内核模块,落盘体积通常 60-90MB,跟 128MB
预算冲突。装机前先确认你这台 VPS 是 LXC/OpenVZ 容器,不是 KVM。

这个目标**也只在 Alpine 上成立**,不是随便换个发行版都行:Debian/Ubuntu
系统身的 OS 基础占用就有几十到上百 MB(本文第一节的 2GB 盘场景假设
"系统 + RustPanel ~600MB" 就是这个量级),跟 128MB 预算差了 4-5 倍。
Alpine 的 minirootfs 本身只有 ~10MB。

### 8.2 实测数据

详见 `docs/guides/alpine-128mb-feasibility.md`(Milestone 0 可行性验证的
完整记录),这里摘要结论:

| 项目 | 实测值 | 对应 128MB 预算的占比 |
|------|-------|----------------------|
| 磁盘:OS + OpenRC + OpenSSH + chrony + sqlite + 面板二进制 + 最小运行态 | ~37-38MB | ~30% |
| 内存:空闲 | 4.9MB | ~4% |
| 内存:轻负载(50 次 HTTP 请求)后 | 9.1MB | ~7% |

**没有压测过的场景**(留给以后端到端验证时补):ACME 证书签发、WebSocket
终端会话、zip 打包这几个相对吃内存的操作;真实 OpenRC-as-PID1 的开机自启
/ 崩溃恢复行为(本次实测用 Docker 近似,不是真 LXC)。

### 8.3 装机方式

跟本文前面的 2GB 盘场景共用同一个 `install.sh`,区别只是机器形态不同、
装机器自己会识别:

```sh
curl -fsSL https://example.com/install.sh | bash -s -- \
  --profile micro \
  --public-host <对外 IPv4 或域名>
```

- `detect_distro()` 探测到 Alpine 后自动下载 musl 静态二进制
  (`rustpanel-backend-micro-linux-musl-amd64.tar.gz`),不是 glibc 版本。
- 资源硬门禁对 Alpine 单独放宽:磁盘 100MB / 内存 64MB(Debian 系仍是
  500MB / 80MB)——数字定得比实测结果留了余量,不是贴着 37MB/9MB 这两个
  数字定的,给站点数据、日志、证书留空间。
- 没有 systemd:面板的"节俭模式"(空闲挂起)靠 `rustpanel-backend
  --activate` 这个可移植监督者实现,由 OpenRC 的 `supervise-daemon` 托管,
  效果跟 systemd 的 `.socket` unit 等价,但只需要管一个进程。
- 证书续签、告警扫描都走 cron(`/etc/cron.d` 或 busybox crond 兼容的
  root crontab 回退),没有 systemd timer 可用。

### 8.4 已知局限

- **软件商店在这个磁盘预算上意义不大**:Redis/PostgreSQL 这类原生包装了
  也装不下多少数据,`apk` 后端这次只做了基础的装/卸/升级,没有照搬 apt 那套
  nginx.org 官方源 + pinning 逻辑——Alpine 上的 `nginx-mainline` 模板退化成
  Alpine 自带仓库的 nginx,没有 HTTP/3(`http_v3_module`)。
- **RHEL 系(Rocky/Alma/CentOS)和 Arch 不支持**:这两家默认带 SELinux
  (非标准端口监听需要 `semanage port`)和 firewalld,是独立的工程量,
  这次不在范围内,以后有需求再做。
- 本节内容没有在真实 Alpine LXC 容器里跑过完整的端到端安装(建站点 /
  ACME 签发 / 重启验证开机自启这些),只验证到"各个独立环节在真实 Alpine
  容器里行为正确"这一步——第一次在生产 LXC 容器上装之前,建议先在测试机
  上走一遍完整流程。

## 八、相关 commit 参考

- `b998aa0` —— Phase G 5 个 Rust 栈包模板入软件商店
- `ba74841` —— InstallPlan 数据契约 + Phase G versions + 描述里的官网
- `53f07da` —— `AppTemplate.homepage` proto 字段全数据化
- `f6ada66` —— BinaryDownload executor(curl + tar + systemctl)
- 本文档随后续 commit 落地
