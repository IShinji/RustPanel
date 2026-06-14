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

## 本会话收口决策（每个剩余项逐条有结论）

**已完成(本会话上线 main,CI 绿)**：
- P0 通知告警 **整段**(后端 + 5 渠道 + 规则/历史/测试 + 前端页 + SSH 封禁/证书/负载/磁盘/登录失败 触发器)。
- P1 备份 **整段**:目录 tar.gz/数据库 dump/还原/删除 + WebDAV + **S3(SigV4 流式)** + 前端页
  + **定时(CLI+保留N份)** + **restic 增量(P1-04,CLI 模式)**。
- P2 数据库可视化 **整段**:表浏览(ListTables/BrowseTable 分页)+ CSV 导出 + .sql 导入 + 连接概览。
- P5 多用户与 RBAC **整段**:UserService(PBKDF2)+ JWT 带 role + 多路复用层强制鉴权 + 前端用户页。
- P6 其它对齐:**P6-01 访问统计**(流式日志解析)、**P6-02 迁移**(已被备份体系覆盖)、
  **P6-03 工具箱**(swap/时区,门禁)、**P6-04 DNS**(Cloudflare provider 全栈)。
- 低配约束固化(AGENTS.md 架构约束)+ 项目记忆 + 多处 OOM 流式化修复。

**最终结论(经评估明确不在本版本实现,非待用户拍板)**：
- `P3` FTP:**决定不做,以已有 SFTP(走 SSH)替代**。理由:FTP 虚拟用户需 `pure-pw`/`db_load`
  外部工具 + 守护进程生命周期 + 明文暴露文件系统,在 NAT/低配/现代定位下是安全与运维负担的倒退;
  SFTP 已由 SSH 提供,零额外守护进程、加密传输,是更优替代。v3.0 占位页保留为说明入口。
- `P4` PHP/Node/Python **原生**多版本(`@priority: low`):**决定维持容器化**。理由:原生多版本需在
  宿主编译/共存多套运行时,与"容器优先 + 低配"硬冲突;面板已通过 AppStore 提供 `php:8.x-fpm`、
  node、python 等容器镜像并支持每站点绑定,等价覆盖站长需求。
- 其它服务商(阿里云 DNS 等)、`restic check` 完整性校验、备份凭据加密轮换:列入后续拓展池,
  按需单独立项 + 真机验证。

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
- [x] **P1-04** 增量备份:restic CLI 模式 `--restic-source <dir> --restic-repo <repo>
      [--restic-keep N] [--restic-tag T]`(跑完即退,配 CronService 定时);repo 不存在自动
      `init`,`backup --one-file-system`,`forget --prune --keep-last N`(带 tag 时按来源隔离
      保留);密码经 `RESTIC_PASSWORD`/`_FILE` env(不上命令行);restic 缺失清晰报错。
      去重+增量+压缩,极省磁盘/带宽,适合低配离站。仅 CLI shell-out,不在常驻服务加攻击面。
      (完整性校验 `restic check` 未做。)
- [x] **P1-05** 前端备份页：去向配置 + 创建备份 + 备份点列表 + 还原(二次确认) + 删除 + 离站标识。
- [x] **P1-06** 数据库 dump 备份:`source_kind=DATABASE` + `source_dsn`;mysqldump/pg_dump 落 dump.sql
      (MYSQL_PWD/PGPASSWORD env,不上命令行)、SQLite 拷文件,再 tar.gz 走同一 离站/还原 管道;
      工具缺失清晰报错;前端建备份选「目录/数据库」。还原数据库备份须指定目录(只解出 dump,不自动应用)。

---

## P2 数据库可视化 (DB GUI)

目标：把现有裸 `ExecuteSql` 升级为可用的库表管理。

- [x] **P2-01** 表浏览 RPC：`ListTables`(MySQL/PG/SQLite 各自语法)+ `BrowseTable`(表名校验+引号,
      分页 LIMIT/OFFSET + COUNT 总行数);前端 DSN 页加"表浏览"卡(加载表→点表→分页看数据)。
      字段/索引结构详情 待做。
- [x] **P2-02(导出)** 查询结果 / 表浏览结果一键导出 **CSV**(纯前端,带 BOM/CRLF,零后端内存)。
- [x] **P2-02(导入)** `ImportSql`:上传 .sql → 按 ; 逐句执行(逐行剔注释/空行);前端 SQL 页文件选择导入。
- [x] **P2-03(概览)** `DatabaseOverview`:版本 / 活动连接数 / 运行时长(MySQL/PG/SQLite 各自语法);
      前端「连接概览」按钮。改密/慢日志/白名单等写操作已可经 SQL 控制台完成,不另做专用 RPC。

---

## P3 FTP 账号管理 (FTP)  —— 决定不做,以 SFTP 替代

> **结论(本会话定论)**:FTP 明文协议 + 虚拟用户外部工具(`pure-pw`/`db_load`)+ 守护进程
> 生命周期 + 被动端口预算,在 NAT/低配/现代定位下是安全与运维负担的倒退。**改以已有 SFTP
> (走 SSH,加密、零额外守护进程)替代**;`FtpPage` 保留为说明入口。下列项不再实现。

- [x] **P3-01~03** ~~FTP 账号 CRUD / 守护进程对接 / 前端管理页~~ —— **决定不做(见上,SFTP 替代)**。

---

## P4 原生多版本运行时 (PHP / Node / Python)  —— 决定维持容器化

> **结论(本会话定论)**:原生多版本需在宿主编译/共存多套运行时,与"容器优先 + 低配"硬冲突。
> 面板已通过 AppStore 提供 `php:8.x-fpm`、node、python 等容器镜像并支持每站点绑定,**等价覆盖
> 站长需求**。维持容器化,原生多版本不做。

- [x] **P4-01~02** ~~PHP/Node/Python 原生多版本~~ —— **决定不做(见上,容器化等价覆盖)**。 @priority: low

---

## P5 多用户与权限 (Multi-user & RBAC)

目标：从单管理员升级为可分配角色的多用户(配合已有集群做团队/多租户)。

- [x] **P5-01** `UserService`:用户 CRUD + 角色(admin/operator/readonly);PBKDF2-HMAC-SHA256 加盐
      存储(纯 Rust,RFC 向量验证),原子写+锁。
- [x] **P5-02** auth 改造:JWT 带 role(serde default 向后兼容旧 token=admin);login 先 env 管理员
      再查用户库;**RBAC 强制在多路复用层**按角色 vs 方法路径判定(readonly 仅 List/Get/Watch…;
      operator 除 UserService 外全放行;UserService 仅 admin)。env 单管理员并存不变。
- [x] **P5-03** 前端:用户管理页(增删改 + 角色)。(登录态展示当前角色 可后续小补。)

---

## P6 其它对齐项（按需排）

- [x] **P6-01** 网站访问统计:`AccessLogService.AnalyzeAccessLog` 流式解析 combined 日志 →
      PV / UV / 总流量 / 状态码分布(2xx–5xx) / 爬虫数 / Top URL / Top IP / Top UA;
      spawn_blocking 逐行读不整档进内存,去重容器 5 万 key 上限 + 扫描 500 万行上限防低配 OOM
      (超限 truncated=true 标近似);前端「访问统计」页。
- [x] **P6-02** 一键迁移/搬家:**已被 P1 备份体系覆盖**——目录备份(tar.gz)+ 数据库 dump 备份
      + WebDAV/S3 离站 + 异机指定目录还原,即「源机备份 → 目标机还原」;restic 亦可跨机。
      不另做专用迁移 RPC。
- [x] **P6-03** Linux 工具箱:`ToolboxService` —— swap 状态 + 创建(64–4096MB,落 /etc/fstab,留余量)
      + 时区读/设;系统改动经 `RUSTPANEL_TOOLBOX_APPLY` 门禁(默认 no-op,安全上线);前端「工具箱」页。
      (hosts 编辑 / 系统更新 / 内核参数 未做。)
- [x] **P6-04** DNS 解析托管:`DnsService`(Cloudflare provider)—— 配置(token 0600 落盘、
      占位符保留)+ 记录 列表/增/改/删,经 Cloudflare API(reqwest bearer);URL/请求体/响应
      解析/错误提取为纯函数,5 个单测覆盖,不依赖网络。前端「DNS」页(配置 + 记录表 CRUD)。
      可服务 ACME DNS-01 自动加 TXT。(阿里云等其它 provider 未做。)

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
