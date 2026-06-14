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
- [x] **P0-02-1** 证书到期扫描：`spawn_alert_scanner` 周期任务,`ssl::cert_expiry_overview()` 剩余 ≤ 阈值即推送 `CERT_EXPIRY`;内存去重 24h。
- [x] **P0-02-2** 高负载 / 磁盘将满：sysinfo 负载(按核折算%)/各挂载点占用,超阈值推送 `HIGH_LOAD` / `DISK_FULL`;去重 1h。
- [x] **P0-02-3** 异常登录：auth 登录失败内存滑窗(600s/5 次)聚合后推送 `LOGIN_FAILED`(默认关,1h 冷却)。

### P0-03 前端通知设置页
- [x] **P0-03-1** 新增「通知」页(系统组,独立导航):渠道增删改 + 一键测试 + 事件规则开关/阈值 + 发送历史。

---

## P1 备份体系 (Backup & Restore)  —— 面板用户底线诉求

目标：网站 / 数据库可定时备份到本地与云端，并能一键还原。

- [x] **P1-01** `backup.proto` + `backup.rs`：目录备份(tar.gz)、记录列表、安全还原(unpack_in 防 tar-slip)、删除;原子写+锁。**(数据库 dump 备份待做,见 P1-06)**
- [x] **P1-02** 云存储 target：WebDAV + **S3 兼容(R2/MinIO/OSS/COS,SigV4 + 路径风格 +
      UNSIGNED-PAYLOAD 流式上传)** 均已实现;前端去向表单含 S3 region/bucket。
      (凭据落盘加密 待做,见拓展池;真实 S3 端到端以 MinIO/R2 实测为终检。)
- [x] **P1-03** 定时策略：CLI 一次性备份模式 `rustpanel-backend --backup-source <dir>
      [--backup-target <id>] [--backup-keep N]`(跑完即退),配 CronService 即可定时;
      `--backup-keep` 实现"保留同源最新 N 份"。(cron 页一键预设模板 待补。)
- [ ] **P1-04** 增量与校验：restic 集成(P8 catalog 已含 restic)做增量 + 完整性校验。
- [x] **P1-05** 前端备份页：去向配置 + 创建备份 + 备份点列表 + 还原(二次确认) + 删除 + 离站标识。
- [ ] **P1-06** 数据库 dump 备份:mysqldump / pg_dump 落 .sql 后并入归档(扩展 source_kind)。

---

## P2 数据库可视化 (DB GUI)

目标：把现有裸 `ExecuteSql` 升级为可用的库表管理。

- [x] **P2-01** 表浏览 RPC：`ListTables`(MySQL/PG/SQLite 各自语法)+ `BrowseTable`(表名校验+引号,
      分页 LIMIT/OFFSET + COUNT 总行数);前端 DSN 页加"表浏览"卡(加载表→点表→分页看数据)。
      字段/索引结构详情 待做。
- [x] **P2-02(导出)** 查询结果 / 表浏览结果一键导出 **CSV**(纯前端,带 BOM/CRLF,零后端内存)。
- [ ] **P2-02(导入)** 上传 .sql 导入(需按引擎拆语句 / batch execute) 待做。
- [ ] **P2-03** MySQL/PG 运维：root/超级用户改密、慢日志开关、远程访问白名单、连接数概览。

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
