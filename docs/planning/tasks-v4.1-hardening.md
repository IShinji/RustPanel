# v4.1 加固与工程质量收口

v4.0 的功能任务全部收口后做的一次全仓审查,针对的是「功能齐了、但违反自己定的
安全与低配约束」的存量问题。本版不加新功能。

## 执行规则

- 按 `blocker → high → medium → low` 执行。
- 完成后将 `[ ]` 改为 `[x]`；阻塞任务保持 `[ ]` 并加 `@status: BLOCKED`。
- 每个阶段结束时系统须可编译、`./scripts/verify-all.sh` 通过。

---

## P0 安全

- [x] **P0-01** 面板登录限速。此前 `login` 对失败只发告警(默认还关着),没有任何锁定,
      面板暴露公网时爆破只受 PBKDF2 计算成本限制;而 128MB / 单核小鸡上,十万轮
      PBKDF2 被刷起来本身就是 CPU DoS。现按账号计失败次数,达阈值指数退避锁定
      (默认 5 次 / 60s 起,封顶 900s),**判定在校验口令之前**,锁上了一轮哈希都不跑。
      表容量封顶 512 条防用户名喷洒撑爆内存。可用 `RUSTPANEL_LOGIN_MAX_FAILURES` /
      `RUSTPANEL_LOGIN_LOCK_SECONDS` 调整。
      *(没有客户端 IP 可用:多路复用层是 `Shared` service,拿不到 peer addr。按用户名
      计数是当前结构下的最优解;若日后要按 IP,需要改成
      `into_make_service_with_connect_info` 并把 ConnectInfo 透传进 tonic。)*
- [x] **P0-02** 机密文件 0600 落盘。全仓 27 处 tmp+rename 只有 acme 私钥和 dns token
      设了权限,其余按 root 默认 umask 落成 0644 —— 本机任意用户/容器可读。新增
      `statefile::{write_atomic, write_secret_atomic}`,权限在 rename **之前**打到 tmp 上,
      目标文件不留宽权限窗口。改为机密落盘的有:备份去向(S3 secret key / WebDAV 口令)、
      通知渠道(bot token / webhook URL)、用户库(PBKDF2 口令哈希)、ACME pending
      (账号私钥)、集群 node_secret。
- [x] **P0-03** 令牌吊销。JWT 自包含且角色写死在 token 里,改角色 / 改密码 / 删用户后
      旧 token 在 TTL(默认 24h)内依然全权有效,降权和封禁等于不生效。新增按用户名的
      吊销水位表(`iat <= revoked_at` 即作废),多路复用层与 `token_refresh` 各查一次,
      常驻内存缓存不给每个请求加磁盘读;`token_refresh` 也不再盲信 token 里的角色,
      改从用户库取当前值。
      *(JWT `iat` 只有秒精度:同一秒内「改角色 + 重新登录」会连新 token 一起判掉,
      用户重登一次即可。宁可多拒一秒,不漏放一个降权前的 token。)*

## P1 低配契约(AGENTS.md 架构约束)

- [x] **P1-01** `ReadFile` 整档进内存且无上限,tonic 出站默认不限大小 —— 文件管理器里点开
      几百 MB 的日志直接 OOM。现在先 stat,超过 2MB 拒绝并提示改用流式下载。
- [x] **P1-02** `SearchFiles` 在 async handler 里同步跑 walkdir + 对每个文件 `read_to_string`:
      既整档进内存又占住 reactor 线程,一次全盘检索能让整个面板卡死。改为
      `spawn_blocking` + `BufReader` 逐行读,跳过 >4MB 文件,遍历条目封顶 20 万。
- [x] **P1-03** 静态资源每个请求 `into_owned()` 克隆(单个 vendor chunk 350KB),且无压缩、
      无 ETag。改为 `Bytes::from_static` 零拷贝 + ETag/304 + 惰性 gzip 缓存。
- [x] **P1-04** 前端首屏把 recharts / xterm / monaco 全静态 import 一起下发。拆成三个 lazy
      模块,首屏从约 1.5MB 降到约 843KB(配合 gzip 实际传输约 224KB)。
- [x] **P1-05** cron / capability / proxy / workload 的状态保存此前是裸 `write`(非原子),
      崩溃或并发会留半截 JSON 让对应模块永久 500。收敛到原子写助手。

## P2 工程质量

- [x] **P2-01** Web 侧 `bun lint` 此前只是 `tsc --noEmit`。接入 ESLint(typescript-eslint +
      react-hooks),修掉 4 个新暴露的问题。剩 7 条 hooks 依赖 warning 是刻意的挂载即取,
      按 warn 记账不阻断。
- [x] **P2-02** 测试:后端 145 → 172,web 3 → 16。重点补此前 0~1 个测试的 toolbox /
      workload / proxy / cron,以及备份、通知、数据库三条端到端链路(见 P3)。
- [x] **P2-03** 拆分 10071 行的 `App.tsx` → 1610 行 + 8 个页面模块 + 6 个共享模块。
- [x] **P2-04** 清掉 admin 死脚手架:`admin-deploy.yml`(指向不存在的 `src/admin`,无人调用)、
      verify-all.sh 空分支、两个版本脚本里的 admin 路径、两份占位 schema 文档。
- [x] **P2-05** 文档:README 路线图补 v4.0 与验证现状;`docs/guide/` 并入 `docs/guides/`。

## P3 v4.0 验证计划的本地闭环

v4.0 的 V-02 / V-03 / V-04 原本整条挂在「需要真机」上。这次把**不需要外部服务的那部分**
用测试固化下来,真机验证的范围随之缩小到只剩「外部服务」本身:

- [x] **P3-01(V-02 本地部分)** 起本地 HTTP 接收端跑通 webhook 派发:payload 形状、
      Bearer 注入、无 secret 不带 Authorization、非 2xx 记失败、停用渠道只在测试发送时才碰。
- [x] **P3-02(V-03 本地部分)** 备份 → 删源 → 还原回原位内容一致;还原到异地目录
      (异机还原的等价路径);未知去向报 NotFound;数据库备份必须显式指定还原目录。
- [x] **P3-03(V-04 本地部分)** SQLite 端到端:ImportSql 建表插数 → ListTables →
      BrowseTable 分页与总行数 → ExecuteSql 查询/影响行数 → DatabaseOverview;
      外加表名注入串必须被拒且库表无损。

## 仍需真机 / 外部服务(非代码任务)

- [ ] **V-02(远端部分)** 真实 Telegram / 钉钉 / 企业微信 / Bark 端点各发一次。
- [ ] **V-03(远端部分)** WebDAV 与 S3 兼容去向用 MinIO / R2 实测上传下载删除;
      restic 增量用真实 repo 跑一轮 init / backup / forget --prune。
- [ ] **V-04(远端部分)** MySQL / PostgreSQL 实例上重跑 P3-03 的同一套往返。
- [ ] **V-05** 低配真机(OpenVZ / ~128MB)冒烟:安装器 micro 档、能力探针置灰、
      静态站 + 反代 + 原生进程主线。

## 后续拓展池(沿用 v4.0)

- 通知:邮件(SMTP)、Server酱、飞书;去重 / 静默窗口 / 升级策略。
- 备份:`restic check` 完整性校验、备份凭据加密与密钥轮换、增量快照浏览。
- 可观测:Prometheus 导出端点、Alertmanager 接入。
- 安全:按客户端 IP 的登录限速(需要 ConnectInfo 透传,见 P0-01 注)。
- 前端:7 条 hooks 依赖 warning 逐个消化;页面级 lazy 路由(目前只有重依赖 lazy)。
