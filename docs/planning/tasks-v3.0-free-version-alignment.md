# RustPanel 任务清单 v3.0 - 与宝塔免费版功能对齐

更新日期：2026-05-09

## 版本目标

补齐 RustPanel 在**可视化安全、进阶站点、自动化运维、监控历史**方面与宝塔免费版的代差。实现真正“开箱即用”且具备“生产防御能力”的运维面板。

## 对标参考：宝塔面板核心功能深度解析
 
### 1. 监控历史与性能看板 (Monitoring & History)
* **多维度指标可视化**：以折线图形式实时及历史展示：
    * **系统负载**：1/5/15 分钟负载趋势。
    * **核心资源**：CPU 使用率（分核心）、内存占用（已用/缓存/空闲）。
    * **磁盘 I/O**：实时读写速度、I/O 等待/延迟。
    * **网络流量**：上行/下行实时带宽、累计入流量/出流量。
* **回溯与深度分析**：
    * **时间筛选**：支持今天、昨天、最近 7 天及自定义任意时间段回溯。
    * **异常定位**：支持在图表上通过交互定位特定时刻的资源异常。
* **数据管理与报表**：
    * **存储策略**：支持设置数据保存天数（默认 30 天），查看日志占用空间。
    * **维护工具**：提供一键清理过期时序数据。
    * **运行周报/日报**：自动生成汇总报告，涵盖安全风险、资源峰值及健康度分析。

### 2. 站点管理与流量转发 (Web & Proxy)
* **项目全生命周期**：
    * **技术栈分类**：支持 PHP, Java, Node, Go, Python, .Net, HTML 分类管理。
    * **基础配置**：多域名绑定、子目录设置、根目录变更、防盗链设置。
    * **进阶控制**：WAF 总开关、IP 黑白名单、伪静态（内置 WordPress/Laravel 等模板并支持在线编辑）。
* **SSL 证书全自动维护**：
    * **实时监控**：Dashboard 显著的过期预警提示，统一视图展示所有证书到期天数。
    * **获取方式**：Let's Encrypt 一键免费申请、手动导入商用/自备证书（CRT/KEY）。
    * **自动化闭环**：到期前 30 天自动续签，续签成功后静默执行 Nginx 重载。
* **反向代理管理**：
    * **图形化配置**：支持为非 PHP 项目创建转发规则，支持多规则管理。
    * **高级参数**：内置负载均衡策略、缓存配置、连接频率限制。
    * **状态管理**：一键开启/关闭转发，实时监控代理目标可用性。

### 3. 数据库管理系统 (Database Management)
* **全栈支持**：MySQL, SQLServer, PgSQL, SQLite, MongoDB, Redis 一站式管理。
* **运维操作**：
    * **自动化部署**：环境一键安装、多版本并存切换。
    * **账号权限**：数据库一键创建、用户授权、远程连接白名单控制、权限同步。
* **灾备与迁移**：
    * **自动备份**：支持配置定时备份策略（按天/按周/按频率）。
    * **同步工具**：支持本地同步、远程数据库接入、大规模数据迁移及一键导入导出。
* **可视化增强**：内置 phpMyAdmin 等工具跳转入口，支持可视化 SQL 编辑与表结构操作。

### 4. 系统安全矩阵 (Security Matrix)
* **可视化防火墙**：
    * **端口管理**：精确放行/屏蔽端口规则（TCP/UDP/ICMP），支持备注说明。
    * **系统级防御**：禁 Ping 设置、端口防扫描、防火墙全局开关、规则导入导出备份。
    * **来源过滤**：支持针对特定 IP 或 IP 段配置防火墙规则。
* **WAF (Nginx 防火墙)**：
    * **攻击拦截**：抗 CC 攻击（验证码挑战/频率限制）、常见 Web 漏洞防护、恶意扫描拦截、敏感关键词过滤。
    * **攻击分析**：全球攻击来源地图、24小时/7天攻击趋势图、IP 攻击排行及地理分布统计。
    * **规则下发**：支持全局通用规则与站点级差异化防御规则。
* **SSH 安全管理**：
    * **加固配置**：支持自定义 SSH 端口、服务一键开关、密钥管理（RSA/Ed25519）及禁用密码登录。
    * **日志审计**：实时监控并记录 SSH 登录成功/失败轨迹，支持攻击 IP 自动封禁。
* **进阶加固**：集成入侵防御（关键文件篡改监控）、扫描感知、恶意 IP 实时库。

### 5. 容器云管理 (Docker)
* **全生命周期控制**：图形化启停、重启、暂停、删除、批量处理，支持资源配额（CPU/内存）限制。
* **运维与调试**：
    * **内置交互**：一键进入容器内部 Web 终端、实时滚动日志流、容器性能指标图表。
* **镜像与编排**：
    * **镜像仓库**：镜像拉取、构建、私有仓库管理、版本回滚。
    - **Compose 编排**：图形化 Docker Compose 部署与可视化编排管理。
    - **网络与存储**：虚拟网络分配、持久化卷挂载管理。
* **安全清理**：一键清理冗余资源（孤立镜像/卷），内置容器安全漏扫。

### 6. 图形化文件管理 (File Manager)
* **基础文件操作**：CRUD、批量处理、拖拽上传、大文件断点续传/下载、多格式压缩包管理。
* **导航与定位**：面包屑路径导航、常用项目路径收藏夹。
* **开发者专用**：集成在线代码编辑器（语法高亮/热更）、目录级 Web 终端快捷入口。
* **内容检索**：支持全局或目录级的内容全文搜索（支持正则表达式）。
* **权限与安全**：图形化修改权限/所有者、回收站机制（防误删还原）、核心文件防篡改监控。

### 7. 日志审计中心 (Logs)
* **全行为审计**：
    * **操作追踪**：详细记录用户、模块、具体动作描述、时间戳、操作来源 IP。
    * **多维度分类**：面板操作日志、登录历史、系统运行日志、计划任务执行日志。
* **维护与分析**：
    * **运维工具**：实时刷新显示、关键词全文检索、日志一键清理归档。
    * **智能分析**：IP 操作频率统计报表、AI 自动化日志风险分析。

### 8. 计划任务与自动化 (Automation)
* **任务调度**：支持 Shell 脚本、站点/数据库定时备份、日志自动切割、系统内存释放。
* **生命周期控制**：任务手动执行验证、暂停/启用控制、执行日志回溯、上次运行时间展示。
* **扩展能力**：内置常用运维脚本库、支持任务规则批量导入导出。

### 9. 域名、集群与软件生态 (Assets & Cluster)
* **资产管理**：域名一键注册、实名认证、DNS 解析托管、合规性指引。
* **节点集群**：多机统一运维、配置/文件一键分发、图形化负载均衡方案、主从同步、多机异常告警。
* **软件商店**：运行环境（Nginx/MySQL/PHP/Node 等）一键安装、多版本并存切换。内置常用开源程序一键部署。

### 10. 面板设置与控制中心 (Settings)
* **访问控制**：自定义面板端口、唯一安全路径。支持域名绑定、面板 SSL 开启、2FA 两步验证、IP 白名单。
* **个性化定制**：侧边栏菜单按需隐藏、多语言切换、自定义 Dashboard。
* **灾备迁移**：配置备份还原、自动化整机环境迁移工具。


## 执行规则

- 按 `blocker → high → medium → low` 执行。
- 完成后将 `[ ]` 改为 `[x]`。
- 阻塞任务保持 `[ ]` 并添加 `@status: BLOCKED`。

---

## P1 第一步：安全底座与防火墙强化 (Security & Firewall)
 
目标：可视化接管网络安全，实现具备 WAF 防御与 SSH 加固的生产级安全环境。
 
### P1-01 系统防火墙与入口安全
- [x] **P1-01-1** 扩展 `security.proto`：支持 TCP/UDP/ICMP 协议、备注、来源 IP/IP 段限制。
- [x] **P1-01-2** 后端实现防火墙适配层：支持 UFW/Firewalld/Iptables，增加“禁 Ping”与“防扫描”逻辑开关。
- [x] **P1-01-3** 开发“安全管理”页面：实现规则增删改查、规则导入导出备份。
- [x] **P1-01-4** 实现面板“安全入口”自定义：支持自定义访问路径、服务端口，并实现 2FA 两步验证登录。
 
### P1-02 WAF Web 应用防火墙 (Nginx 层)
- [x] **P1-02-1** 实现抗 CC 攻击逻辑：集成 Nginx 频率限制模块，支持验证码挑战页面分发。
- [x] **P1-02-2** 实现漏洞防护与关键词拦截：预设 SQL 注入、XSS 攻击过滤规则库。
- [x] **P1-02-3** 攻击可视化看板：前端开发“全球攻击来源地图”与攻击 IP 排名统计表。
 
### P1-03 SSH 专项加固
- [x] **P1-03-1** 实现 SSH 端口修改 RPC 及后端配置重载。
- [x] **P1-03-2** 登录审计与防护：记录登录成功/失败轨迹，支持多次失败尝试后自动封禁攻击源 IP。
- [x] **P1-03-3** 密钥管理：实现在线生成 RSA/Ed25519 密钥对，一键禁用密码登录。
 
---
 
## P2 第二步：进阶站点管理与 SSL (Advanced Web & SSL)
 
目标：实现全自动 SSL 维护与复杂的流量转发控制。
 
### P2-01 SSL 证书自动化闭环
- [x] **P2-01-1** 实现 SSL 过期预警机制：后台定时扫描证书，并在过期前 30/7/1 天推送预警。
- [x] **P2-01-2** 完善证书续签与静默部署：集成 ACME 协议，支持到期自动续签并自动重载 Nginx 证书配置。
- [x] **P2-01-3** 证书管理 UI：支持分组管理、商用证书手动导入、到期时间统一视图。
 
### P2-02 伪静态、反代与负载均衡
- [x] **P2-02-1** 伪静态模板库：内置 WordPress/Laravel/ThinkPHP 等模板，支持在线编辑器。
- [x] **P2-02-2** 图形化反向代理管理：支持多规则配置、缓存设置、连接频率限制。
- [x] **P2-02-3** 负载均衡 UI：支持在面板端配置多目标主机的 upstream 负载均衡方案。
 
---
 
## P3 第三步：图形化文件运维 (Files & Developer Tools)
 
目标：提供“零命令行”的文件管理与代码编辑体验。
 
### P3-01 进阶文件操作
- [x] **P3-01-1** 拖拽上传与大文件管理：实现基于 Chunk 的大文件断点续传。
- [x] **P3-01-2** 实现文件“回收站”：删除时逻辑移动，支持一键还原或彻底清空。
- [x] **P3-01-3** 文件安全审计：实现核心文件防篡改实时监控，记录文件修改权限/所有者行为。
 
### P3-02 开发者效率工具
- [x] **P3-02-1** 在线代码编辑器强化：支持多语种语法高亮、快捷保存。
- [x] **P3-02-2** 目录级 Web 终端：实现从文件管理器一键开启对应路径的 xterm 终端。
- [x] **P3-02-3** 全局内容检索：后端集成搜索接口，支持在指定目录下进行跨文件全文搜索（正则支持）。
 
---
 
## P4 第四步：时序监控与运行报告 (Monitoring & Reports)
 
目标：实现可溯源的性能看板与自动化体检。
 
### P4-01 历史数据分析
- [x] **P4-01-1** 历史趋势回溯：Dashboard 支持查看 1h, 24h, 7d 及自定义时间段的指标曲线。
- [x] **P4-01-2** 异常指标定位：在图表上通过交互直接显示特定时刻的进程资源详情。
- [x] **P4-01-3** 自动化日报/周报：定时生成系统健康报告（含安全拦截数、资源峰值摘要）。
 
---
 
## P5 第五步：容器管理与软件生态 (Docker & App Store)
 
目标：将 Docker 作为核心运行底座，实现应用的一键全自动托管。
 
### P5-01 Docker 深度接管
- [x] **P5-01-1** 资源配额管理：支持通过 UI 设置容器的 CPU/内存硬限制。
- [x] **P5-01-2** Docker Compose 图形化编排：支持在线编辑 compose 文件并可视化管理项目组。
- [x] **P5-01-3** 镜像安全管理：实现镜像拉取进度展示、版本回滚及残留资源一键清理。
 
### P5-02 应用商店与多版本并存
- [x] **P5-02-1** 软件生命周期管理：实现 Nginx/MySQL/PHP 等环境的一键安装、卸载及在线更新。
- [x] **P5-02-2** 多版本切换逻辑：支持同一台服务器运行多个版本的运行环境而不冲突。
 
---
 
## P6 第六步：集群管理与日志审计 (Cluster & Audit)
 
目标：实现多机调度与全行为可追溯。
 
### P6-01 多节点集群运维
- [x] **P6-01-1** 节点接入 RPC：实现分布式节点的秘钥配对与状态心跳。
- [x] **P6-01-2** 统一分发：实现在主控面板一键将文件/配置下发至所有集群节点。
 
### P6-02 全量日志中心
- [x] **P6-02-1** 实现“操作黑匣子”：详细记录登录、文件变动、设置变更的操作详情与来源 IP。
- [x] **P6-02-2** AI 日志分析：集成大模型接口，对异常登录或异常操作日志进行自动化安全解读。
 
---
 
## P7 第七步：UI 信息架构与视觉对齐 (BaoTa-style Layout)

目标：让前端视觉与信息架构与宝塔免费版形成高一致性，提供“看起来就像宝塔”的运维体感。

### P7-01 主题与设计 Token
- [x] **P7-01-1** 双主题（默认亮色 + 暗色可切） + 系统跟随，localStorage 记忆。
- [x] **P7-01-2** 设计 Token 重写：宝塔绿主色、语义色（success/warning/info/destructive）、侧栏专用 token、图表 token、终端专用色。
- [x] **P7-01-3** 遗留 styles.css 全量迁移：所有 hex 硬编码改为 CSS 变量，杜绝“白底白字”对比度问题。

### P7-02 信息架构与导航
- [x] **P7-02-1** 侧栏宝塔分组（总览 / 主机 / 资源 / 安全 / 工具 / 系统），分组标题样式。
- [x] **P7-02-2** 顶栏 Topbar：面包屑 + ThemeToggle + 用户菜单。
- [x] **P7-02-3** Dashboard 改造：服务器信息卡 + CPU/内存/磁盘/网络四联指标卡 + CPU/内存历史折线 + 已安装软件状态。

### P7-03 补缺页面
- [x] **P7-03-1** 面板设置页（基础/安全/SSL/关于 四 Tab），复用 SecurityOptions / SSL 既有 RPC。
- [x] **P7-03-2** 操作日志页（独立 audit Tab，支持模块/关键字过滤）。
- [ ] **P7-03-3** FTP 用户管理：**@status: BLOCKED** —— 后端 FtpService 尚未实现（无 proto/无 backend 模块）。前端已落地占位页（src/web/src/App.tsx 中的 `FtpPage`）引导用户改用文件管理器或 Web 终端 sftp。需在后续版本规划：`proto/rustpanel/v1/ftp.proto`、`src/backend/src/ftp.rs`、容器化 vsftpd/proftpd 守护进程对接。

---

## P8 第八步：受限 VPS 自适应（128MB / 2GB / OpenVZ NAT 友好）

目标：把 RustPanel 的目标用户群从"宝塔克隆"明确切到"NAT VPS / OpenVZ / IPv6 多 IP 这类受限环境的自助管理"。装得下、跑得动比"功能多"更重要。

### P8-01 主机能力探针 + 资源预算（Phase A）
- [x] **P8-01-1** CapabilityService 探针：OpenVZ / overlay2 / FUSE / iptables / nf_nat / swap / BBR / cgroups v2 / user_namespaces / Docker socket，1 小时缓存。
- [x] **P8-01-2** ResourceBudget：实时 RAM / 多挂载点磁盘(simfs 去重) / 1+5+15 负载;NAT 端口预算(默认 20,RUSTPANEL_NAT_PORT_TOTAL 覆盖)。
- [x] **P8-01-3** NAT 端口预留持久化：JSON 文件落到 RUSTPANEL_CAPABILITY_ROOT/ports.json,前端通过专用网络页登记 owner+description+协议。
- [x] **P8-01-4** 公网 IPv6 列举：优先 `ip -6 -o addr show scope global`,回退 `/proc/net/if_inet6`;过滤 link-local/loopback;按 prefix_length 截断推导 /80 公网前缀。
- [x] **P8-01-5** 前端 Dashboard 三预算条 + Network 页(NAT 端口表 + IPv6 池 + 12 项能力探针 Badge)。

### P8-02 软件商店轻量化 + 兼容性筛选（Phase B）
- [x] **P8-02-1** AppTemplate 元数据扩展：CompatibilityStatus / InstallMethod / AppCategory / min_ram_mb / min_disk_mb / expected_runtime_ram_mb / recommended。
- [x] **P8-02-2** 内置 12 个轻量 catalog：nginx-light / caddy / sqlite / redis-tuned / postgres-tiny / hugo / zola / restic / rclone / fail2ban / wireguard / certbot;原 5 个 Docker 模板按宿主能力降级到"需要 Docker"组。
- [x] **P8-02-3** evaluate_compatibility 后端实时判定 + 前端 4 组分类渲染(可用 / 资源不足 / 内核不支持 / 需要 Docker)。

### P8-03 数据库轻量优先（Phase D）
- [x] **P8-03-1** SQLite 文件管理：`list_sqlite_files` 通过 magic bytes 识别,`create_sqlite_file` + WAL,`vacuum_sqlite` 返回压缩前后大小。
- [x] **P8-03-2** Redis INFO 监控：`get_redis_info` 解 12 项关键指标,前端 8 卡片视图含命中率/maxmemory 占比/淘汰策略。
- [x] **P8-03-3** DatabasePanel 重构为 SQLite(默认) / Redis / 通用 DSN 三 Tab,通用 DSN 内嵌原 3 子 Tab 并加"低配优先用 SQLite"提示。

### P8-04 Let's Encrypt + Cron 模板（Phase E)
- [x] **P8-04-1** ACME 挑战类型扩展：`AcmeChallengeType` enum,`RequestCertificateRequest` 增 challenge_type / dns_provider / dns_credentials,Response 增 dns_record_name / dns_record_value。
- [x] **P8-04-2** DNS-01 manual 模式：面板返回 `_acme-challenge.<domain>` TXT 记录提示,用户加 DNS 后再触发验证(NAT VPS 拿不到 80 端口的唯一可行方式)。
- [x] **P8-04-3** Settings → SSL Tab 增"申请新证书"卡(域名/邮箱/挑战类型),展示需要添加的 TXT 记录 + dig 验证提示。
- [x] **P8-04-4** Cron 预设模板下拉:每日 SQLite 备份 / 每周 restic 增量 / 每月 logrotate / 磁盘 80% 告警 / SSL 续期检查 / fail2ban 状态汇报 6 个常用任务一键填表。
- [ ] **P8-04-5** 真实 ACME 客户端集成 + 自动续期定时器：**@status: BLOCKED** —— instant-acme crate 已在 dependencies,但 ACME 完整 order/challenge/finalize 流程未接,目前 manual 模式只生成提示 TXT。需引入异步状态机:第一次请求建 order + 暂存 ACME nonce → 第二次请求拉 token → 写 cert。Cloudflare/Route53 provider API 调用同样未做。

### P8-05 站点模型扩展(Static / Rust / 反代;NAT/IPv6/TLS)— Phase C
- [x] **P8-05-1** site.rs 扩展 SiteKind { STATIC, RUST_BINARY, REVERSE_PROXY } + SiteBinding { NAT_PORT, IPV6_ADDRESS } + SiteTlsStrategy { NONE, LETSENCRYPT_DNS01, IMPORTED };render_phase_c_site 按 kind/binding/tls 输出 nginx vhost(NAT 端口模式 listen <port>; listen [::]:<port>;,IPv6 模式 listen [<addr>]:443 ssl http2;);RUST_BINARY 自动生成 systemd unit 名 + 127.0.0.1:9100+ 内部端口。
- [x] **P8-05-2** SitesSsl 加"新建站点(Phase C · NAT/IPv6 感知)"卡片,3 段表单(类型→绑定→TLS)+ 实时联动 v6 地址池下拉 + 已占 NAT 端口提示;创建后展示生成的 vhost + 自动调 capability.reservePort 锁定端口预算。
- [x] **P8-05-3** 附带修复 IPv6 池误判公网(::2 等 ::/96 deprecated 段,fc00::/7 ULA 等)+ 面板自身监听端口在 NAT 预算上自动登记(seed_panel_port,启动时一次)。

### P8-06 30 秒自动回滚护栏(NO SUPPORT VPS)— Phase F
- [x] **P8-06-1** rollback.rs:RollbackService 三 RPC(ScheduleRollback / ConfirmRollback / ListPendingRollbacks),tokio::spawn + Notify 实现"按时执行 revert_command 或被用户 confirm 取消";actions.json 持久化到 RUSTPANEL_ROLLBACK_ROOT。
- [x] **P8-06-2** 前端 RollbackBanner 全局倒计时横幅(Topbar 下方,3s 轮询),展示最早过期动作的 title/description/进度条 + "保留(我能登录)"按钮,空闲时不显示。
- [ ] **P8-06-3** 把 SettingsPage 改面板端口 / SecurityPanel 改 SSH 端口 / 防火墙规则 apply 三个具体调用点接到 ScheduleRollback。**@status: BLOCKED** —— 需要在 SecurityServiceImpl.UpdateSecurityOptions 内部 spawn 一份 schedule + 应用回滚命令(systemctl restart rustpanel-backend / iptables-restore <snapshot>),涉及 SecurityService 改造,后续单独 PR。当前 RollbackService 已就绪,能脚本化或前端手工调用。

---
 
## 验证计划
 
- [x] **V-01** `./scripts/verify-all.sh` 通过。
- [ ] **V-02** 安全验证：模拟攻击并确认 WAF 拦截记录及攻击地图正确显示。@status: BLOCKED（需要带 Nginx/WAF 的真实运行环境与攻击流量回放）
- [ ] **V-03** 自动化验证：手动修改系统时间模拟过期，确认 SSL 续签脚本自动触发并成功。@status: BLOCKED（需要可变更系统时间的隔离主机，当前本机执行有破坏性风险）
- [ ] **V-04** 集群验证：向节点 A 下发文件，在节点 B 确认接收成功。@status: BLOCKED（需要至少两个已接入的真实集群节点）
- [ ] **V-05** 迁移验证：使用整机迁移工具，确认新服务器环境能完全复刻。@status: BLOCKED（需要目标迁移主机和整机迁移工具链）
