# RustPanel 任务清单 v4.0 - 宝塔差距补齐（运维闭环）

更新日期：2026-06-14

## 版本目标

v3.0 已对齐宝塔在「安全 / 站点 / SSL / 文件 / 监控 / Docker / 集群 / 日志」上的主体功能。v4.0
聚焦 v3.0 **尚未覆盖**、但对任何面板用户都属刚需或高频的运维闭环能力：**通知告警、
备份体系、数据库可视化、FTP、原生多版本运行时、多用户、访问统计**。

保持 RustPanel 既有定位（Rust / 容器 / 静态优先 / NAT 小内存友好），不为「像宝塔」而
牺牲轻量；能容器化解决的优先容器化。

## 现状校准（v3.0 中已被后续提交完成、但清单未更新的项）

- v3.0 `P8-04-5`（真实 ACME 客户端 + 自动续期）：**已完成**（instant-acme 0.8 状态机、
  DNS-01/HTTP-01、续签后自动 reload 反代）。
- v3.0 `P8-06-3`（面板入口变更接 30s 回滚护栏）：**部分完成**——`UpdateSecurityOptions`
  改 listen/path/2FA 已接 `arm_rollback_watchdog`；SSH 端口、防火墙 apply 两个调用点仍未接。
- v3.0 `P7-03-3` FTP：仍 BLOCKED（见本清单 P3）。

## 执行规则

- 按 `blocker → high → medium → low` 执行。
- 完成后将 `[ ]` 改为 `[x]`；阻塞任务保持 `[ ]` 并加 `@status: BLOCKED`。
- 每个 P 级阶段结束时系统须可编译、`./scripts/verify-all.sh` 通过。

---

## P0 通知告警中心 (Notifications)  —— 投入最小、回报最高

目标：把已有的 audit / monitor / SSL 到期 / SSH 封禁等信号，按规则推送到运维常用的
即时渠道。**纯 HTTP 渠道优先**（零 SMTP 依赖）。

### P0-01 通知后端 (NotificationService)
- [x] **P0-01-1** `notification.proto`：渠道 CRUD、设置(按事件开关+阈值)、测试发送、历史列表。
- [x] **P0-01-2** `notification.rs`：渠道持久化(原子写+写锁)，HTTP 分发支持 Webhook / Telegram /
  钉钉 / 企业微信 / Bark；`notify_event` 供其它模块 best-effort 调用。
- [x] **P0-01-3** 注册 `NotificationServiceServer`(走 auth 拦截)。
- [x] **P0-01-4** 触发器：SSH 自动封禁 → `notify_event(SSH_AUTO_BAN)`(spawn，不阻塞封禁路径)。

### P0-02 更多自动触发器（dispatch 管道已就绪，补扫描器即可）
- [ ] **P0-02-1** 证书到期扫描：后台周期任务,到期 ≤ 阈值天推送 `CERT_EXPIRY`(复用 ssl 证书列举)。
- [ ] **P0-02-2** 高负载 / 磁盘将满：复用 monitor collector,超阈值推送 `HIGH_LOAD` / `DISK_FULL`。
- [ ] **P0-02-3** 异常登录：审计 `login_failed` 聚合后推送 `LOGIN_FAILED`。

### P0-03 前端通知设置页
- [ ] **P0-03-1** Settings 新增「通知」Tab：渠道增删改 + 测试按钮 + 事件规则开关/阈值 + 历史。

---

## P1 备份体系 (Backup & Restore)  —— 面板用户底线诉求

目标：网站 / 数据库可定时备份到本地与云端，并能一键还原。

- [ ] **P1-01** `backup.proto` + `backup.rs`：备份任务(站点目录 / 数据库)、备份记录列表、一键还原。
- [ ] **P1-02** 云存储 target：先支持 S3 兼容(MinIO/R2/OSS/COS 均走 S3 API) 与 WebDAV；
      凭据加密存储。
- [ ] **P1-03** 定时策略：复用 CronService,预设「每日库备份 / 每周站点全量 / 保留 N 份」模板。
- [ ] **P1-04** 增量与校验：restic 集成(P8 catalog 已含 restic)做增量 + 完整性校验。
- [ ] **P1-05** 前端备份页：备份点列表 + 还原 + 云目标配置 + 占用统计。

---

## P2 数据库可视化 (DB GUI)

目标：把现有裸 `ExecuteSql` 升级为可用的库表管理。

- [ ] **P2-01** 表/结构浏览 RPC：列库→列表→列字段/索引,分页查行。
- [ ] **P2-02** 导入 / 导出：SQL dump 导出、上传 .sql 导入(复用文件分片上传)。
- [ ] **P2-03** MySQL/PG 运维：root/超级用户改密、慢日志开关、远程访问白名单、连接数概览。
- [ ] **P2-03** 前端：在 DatabasePanel 通用 DSN Tab 下加「表浏览 / 查询器 / 导入导出」子页。

---

## P3 FTP 账号管理 (FTP)  —— 接续 v3.0 P7-03-3

- [ ] **P3-01** `ftp.proto` + `ftp.rs`：FTP 账号 CRUD、目录与配额、启停。
- [ ] **P3-02** 守护进程对接：容器化 vsftpd / pure-ftpd(契合容器优先定位),被动端口走 NAT 端口预算。
- [ ] **P3-03** 前端：替换现有 `FtpPage` 占位页为真实管理页。

---

## P4 原生多版本运行时 (PHP / Node / Python)  —— 视目标市场决定

> 定位权衡：RustPanel 现以**容器**提供 PHP-FPM(`php:8.x-fpm`) 等。若要抢宝塔的 PHP 站长盘,
> 才值得做**原生**多版本;否则维持容器化即可。本 P 默认 `low`。

- [ ] **P4-01** PHP 多版本：原生多版本并存 + 每站点绑定 + php.ini / 扩展管理 + fpm 状态。 @priority: low
- [ ] **P4-02** Node / Python 版本管理 + 项目管理器(常驻进程复用 WorkloadService)。 @priority: low

---

## P5 多用户与权限 (Multi-user & RBAC)

目标：从单管理员升级为可分配角色的多用户(配合已有集群做团队/多租户)。

- [ ] **P5-01** `UserService`：用户 CRUD、角色(admin/operator/readonly)、按模块授权。
- [ ] **P5-02** auth 改造：JWT 带 user+role,Interceptor 做 RBAC 校验;保留单管理员兼容模式。
- [ ] **P5-03** 前端：用户管理页 + 登录态展示当前角色。

---

## P6 其它对齐项（按需排）

- [ ] **P6-01** 网站访问统计：access log 解析 → PV/UV/状态码/Top URL/蜘蛛 报表。 @priority: low
- [ ] **P6-02** 一键迁移/搬家：站点 + 库打包导出,目标机导入(可复用集群文件分发 + 备份)。 @priority: low
- [ ] **P6-03** Linux 工具箱：swap / 时区 / hosts / 系统更新 / 内核参数 一键设置。 @priority: low
- [ ] **P6-04** DNS 解析托管：对接 Cloudflare/阿里云 DNS(亦可服务 ACME DNS-01 自动加 TXT)。 @priority: low

---

## 后续拓展池

- 通知：邮件(SMTP，需引 lettre)、Server酱、飞书;通知去重 / 静默窗口 / 升级策略。
- 备份：增量快照浏览、跨机备份、备份加密密钥轮换。
- WAF：真实攻击地图数据源(GeoIP)、站点级差异化规则下发。
- 可观测：Prometheus 导出端点、告警接入 Alertmanager。

## 验证计划

- [ ] **V-01** `./scripts/verify-all.sh` 通过（每阶段）。
- [ ] **V-02** 通知：配置一个 Webhook(本地 nc/httpbin),触发测试发送与 SSH 封禁,确认收到。
- [ ] **V-03** 备份：备份站点 → 删除 → 还原,内容一致;云 target 用 MinIO 验证。
- [ ] **V-04** DB GUI：建表写数据 → 浏览/导出/导入往返一致。
