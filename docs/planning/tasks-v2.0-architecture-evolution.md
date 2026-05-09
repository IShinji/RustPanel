# RustPanel 任务清单 v2.0 - 现代架构演进与功能闭环

更新日期：2026-05-09

## 版本目标

抛开旧有架构包袱，完全以**架构演进和功能闭环**的逻辑重构/开发 RustPanel。对标宝塔免费功能，但底层全面现代化。采用全栈架构：Rust (Tonic + Axum) + gRPC (Protobuf) + React + Docker 容器化。要求性能最优。

## 执行规则

- 按 `blocker → high → medium → low` 执行。
- 完成后将 `[ ]` 改为 `[x]`。
- 阻塞任务保持 `[ ]` 并添加 `@status: BLOCKED`。
- 强制保证每个 P 级大阶段结束时，系统可编译、可独立运行测试。

---

## P1 第一步：通信底座与系统守护 (Foundation & Daemon)

目标：建立整个面板的骨架，实现极简的单文件部署。

### P1-01 核心通信与 Protobuf 基建
- [x] **P1-01-1** 初始化 `proto` 目录，安装 `protoc` 及 `buf.yaml` 或基础 lint 工具。
- [x] **P1-01-2** 定义 `common.proto`：包含通用的 `Response`（code, message, data）、`Pagination` 及 `Empty` 请求结构。
- [x] **P1-01-3** 定义 `system.proto`：包含基础的 `HealthCheck` 和系统信息探测 RPC 接口。
- [x] **P1-01-4** 配置 Rust 后端 `build.rs`，引入 `tonic-build`，自动将 `.proto` 编译为 Rust 代码。
- [x] **P1-01-5** 配置前端 `buf generate` 脚本，将 `.proto` 编译为 React/TypeScript 可用的 gRPC-web (或 connect-web) 客户端代码。

### P1-02 后端核心架构 (Axum + Tonic)
- [x] **P1-02-1** 初始化 Cargo 后端 Workspace (`cargo init`)，整理工作区目录。
- [x] **P1-02-2** 引入核心依赖库：`tokio`, `tonic`, `axum`, `serde`, `tower` 等。
- [x] **P1-02-3** 集成 `tracing` 与 `tracing-subscriber`，实现后端集中式、结构化日志记录。
- [x] **P1-02-4** 实现 `system.proto` 的 Server Trait（即编写 `HealthCheck` 的具体 Rust 逻辑）。
- [x] **P1-02-5** 建立 `Axum` HTTP Router，作为兜底路由和未来的 HTTP 扩展入口。
- [x] **P1-02-6** 利用 `tower::make::Shared` 实现 gRPC (Tonic) 和 HTTP (Axum) 在同一端口的请求多路复用 (Multiplexing)。

### P1-03 鉴权与安全
- [x] **P1-03-1** 定义 `auth.proto`：包含 Login, Logout, TokenRefresh 等接口。
- [x] **P1-03-2** 引入 `jsonwebtoken`，在 Rust 侧实现强安全的 JWT 签发与解析逻辑。
- [x] **P1-03-3** 编写 `tonic::Interceptor`，对受保护的 gRPC 路由自动校验请求头中的 Authorization Bearer Token，无效则拦截。

### P1-04 静态资源内嵌与守护进程 (Daemon)
- [x] **P1-04-1** 配置 `rust-embed`，映射到前端编译后的 `web/dist` 目录。
- [x] **P1-04-2** 编写 Axum Fallback Route，使其可以响应 `rust-embed` 的静态文件请求（支持 SPA 的 History Router）。
- [x] **P1-04-3** 引入 `clap` 库，设计命令行启动参数（如 `--port`, `--daemon`, `--setup`）。
- [x] **P1-04-4** 实现系统级别的守护进程化逻辑（POSIX Daemonize 或提供一键生成 Systemd service 配置的指令）。

---

## P2 第二步：资源监控与 Web 终端 (Monitoring & Terminal)

目标：实时可视化展示服务器状态，接管底层终端控制权。

### P2-01 系统状态采集推流
- [x] **P2-01-1** 引入 `sysinfo`，封装单例数据采集器，实现对 CPU 核心、内存占用、系统 Load 的低消耗读取。
- [x] **P2-01-2** 封装网络接口读写字节与磁盘 IO 的计算逻辑。
- [x] **P2-01-3** 定义 `monitor.proto`，声明 `ServerStreaming` 类型的 `WatchSystemStatus` 接口。
- [x] **P2-01-4** 在后端利用 `tokio::sync::broadcast` 或通道实现高频（例如 1s 一次）的资源占用数据流式推送。

### P2-02 前端仪表盘 (Dashboard)
- [x] **P2-02-1** 初始化前端 Vite + React + TailwindCSS (或无框架 Vanilla CSS) 架构。
- [x] **P2-02-2** 引入轻量级状态管理 (Zustand/Jotai) 用于接收并全局保存系统流数据。
- [x] **P2-02-3** 集成图表库 (如 Echarts/Recharts)，实现 CPU/内存的动态折线图。
- [x] **P2-02-4** 制作服务器基础信息卡片组件（运行时间、OS 版本、内核版本等）。

### P2-03 Web PTY 后端 (`portable-pty`)
- [x] **P2-03-1** 定义 `terminal.proto`，声明支持双向流 `BidiStreaming` 的终端通信接口。
- [x] **P2-03-2** 引入 `portable-pty`，编写安全派生 Bash/Zsh 伪终端的逻辑，检测系统默认 Shell。
- [x] **P2-03-3** 实现终端输入/输出流与 gRPC/WebSocket 数据通道之间的异步桥接转发 (`tokio::io::copy`)。
- [x] **P2-03-4** 实现终端窗口大小调整 (Resize) 接口，传递列数和行数以触发 SIGWINCH。

### P2-04 Web 终端前端 (`xterm.js`)
- [x] **P2-04-1** 前端引入 `xterm` 核心库及 `xterm-addon-fit` 等扩展。
- [x] **P2-04-2** 封装 React 终端组件，并挂载 gRPC 或 WebSocket 连接流。
- [x] **P2-04-3** 捕获浏览器按键事件并转化为终端字符流下发，处理 Ctrl+C、方向键等。
- [x] **P2-04-4** 监听浏览器窗口 Resize 事件，调用 `fit()` 并在后端触发 PTY 大小同步。

---

## P3 第三步：可视化文件系统 (File Manager)

目标：处理海量文件不卡顿，支持高级文件操作与在线开发。

### P3-01 核心操作与深层遍历
- [x] **P3-01-1** 定义 `fs.proto`，包含目录树获取、新建、移动、重命名、删除、修改权限。
- [x] **P3-01-2** 利用 `tokio::fs` 封装非阻塞的基本文件增删改查 API，必须包含防路径穿越 (Path Traversal) 的安全校验。
- [x] **P3-01-3** 引入 `walkdir` 或 `ignore` 库，实现极速单层或多层目录扫描，支持返回统一的 `FileItem` (大小, 权限, 拥有者, 修改时间)。
- [x] **P3-01-4** 实现 Unix 系统下的 chmod / chown 功能，修改文件属性。

### P3-02 文件传输与归档处理
- [x] **P3-02-1** 实现 HTTP/Axum Multipart 路由，专门用于大文件流式上传 (避免 OOM)。
- [x] **P3-02-2** 实现 HTTP/Axum 路由用于文件下载响应 (Stream response)。
- [x] **P3-02-3** 引入 `zip`, `tar`, `flate2`，在后端封装阻塞打包与解压逻辑（置于 `spawn_blocking` 中执行）。
- [x] **P3-02-4** 为压缩/解压实现任务进度报告机制（通过临时进度队列或 WebSocket 广播）。

### P3-03 前端文件管理器
- [x] **P3-03-1** 搭建经典的文件浏览器 UI（左侧树状结构，右侧列表/网格，上方地址面包屑）。
- [x] **P3-03-2** 实现文件项右键菜单 (Context Menu)，触发重命名、删除、打包、权限修改等 API。
- [x] **P3-03-3** 实现拖拽上传 (Drag & Drop) 及上传进度条 UI 交互。
- [x] **P3-03-4** 集成 `@monaco-editor/react`，实现大文本文件的流畅加载。
- [x] **P3-03-5** 绑定编辑器快捷键 (Ctrl+S / Cmd+S)，自动将修改内容回传服务器保存。

---

## P4 第四步：现代软件商店 (Docker-First App Store)

目标：告别源码编译，全面容器化管理中间件与服务。

### P4-01 Docker 引擎交互 (`bollard`)
- [x] **P4-01-1** 定义 `docker.proto`，包含容器列表获取、状态变更、镜像拉取等协议。
- [x] **P4-01-2** 引入 `bollard` 库，建立与 `/var/run/docker.sock` 的本地通信连接池。
- [x] **P4-01-3** 编写转换层，将 Bollard 的原生模型 (`ContainerSummary`) 转换为 Protobuf 数据格式。

### P4-02 容器操作与日志监控
- [x] **P4-02-1** 封装并暴露容器生命周期 RPC 接口 (Start, Stop, Restart, Remove, Pause)。
- [x] **P4-02-2** 实现 `docker logs -f` 级别的流式日志抓取，将 stdout/stderr 合并后通过 gRPC Stream 推送。
- [x] **P4-02-3** 前端绘制容器列表面板，展示容器名称、端口映射、状态气泡 (Running/Exited)。
- [x] **P4-02-4** 前端实现特定的“容器日志终端”视图弹窗。

### P4-03 编排与预设应用栈
- [x] **P4-03-1** 定义 `appstore.proto`，描述应用市场模板及一键部署接口。
- [x] **P4-03-2** 引入 `serde_yaml`，实现基于官方最佳实践的 `docker-compose.yml` 在线生成（例如自动分配随机密码、设置持久化卷）。
- [x] **P4-03-3** 对接系统的 `docker-compose` 二进制执行或 Bollard 编排逻辑来拉起整个微服务。
- [x] **P4-03-4** 前端实现“应用商店”卡片流 UI（支持一键安装 MySQL, Redis, PostgreSQL 等）。

---

## P5 第五步：站点与反向代理中心 (Web Server & Proxy)

目标：让 Nginx 和 HTTPS 配置变得极其简单且自动化。

### P5-01 站点配置引擎 (`Tera`)
- [x] **P5-01-1** 定义 `site.proto`，支持站点创建、域名绑定、伪静态配置、反向代理目标设置。
- [x] **P5-01-2** 引入 `Tera` (或 Askama)，编写标准且安全的 `nginx.conf` 模板文件（含 Server Block, SSL Block）。
- [x] **P5-01-3** 后端实现接口，根据用户输入参数瞬间渲染 Nginx 配置文件，并保存到 `/etc/nginx/sites-enabled/`。
- [x] **P5-01-4** 实现配置检测与重载机制：利用 `Command` 执行 `nginx -t`，成功后执行 `nginx -s reload`。

### P5-02 SSL 自动化 (`acme-lib`)
- [x] **P5-02-1** 定义 `ssl.proto`，支持自动申请和吊销证书。
- [x] **P5-02-2** 引入 `acme-lib` (或 `instant-acme`)，集成 Let's Encrypt HTTP-01 挑战协议。
- [x] **P5-02-3** 在 Nginx 模板中预设 `.well-known/acme-challenge` 的静态目录转发规则以配合验证。
- [x] **P5-02-4** 实现证书成功签发后的磁盘化保存，并自动触发 Nginx SSL 配置重载。

### P5-03 站点管理前端
- [x] **P5-03-1** 开发站点列表页面，展示绑定域名、运行状态、SSL 到期状态。
- [x] **P5-03-2** 开发建站向导弹窗 (纯静态、反代模式选择，自动关联 P4 拉起的容器)。
- [x] **P5-03-3** 针对 SSL 提供“一键申请”按钮，附带轮询查询/流式更新验证进度的状态展示。

---

## P6 第六步：数据库直连管理 (Database Manager)

目标：剥离 phpMyAdmin，实现内建的极速数据库管控。

### P6-01 异步连接与 DDL (`sqlx`)
- [x] **P6-01-1** 定义 `db.proto`：列出数据库、用户授权、数据备份。
- [x] **P6-01-2** 引入 `sqlx`，封装一个多 DSN 支持的连接池管理器，按需动态连接用户部署的 MySQL/Postgres 实例。
- [x] **P6-01-3** 封装底层的高权限 DDL 语句：支持在界面一键 `CREATE DATABASE`, `CREATE USER`, 并且赋予特定库的访问权限。

### P6-02 Web SQL 与流式处理
- [x] **P6-02-1** 实现 HTTP 流接口，接管 `mysqldump` 的标准输出 (Stdout)，将备份文件以 stream 形式直接传输给浏览器，避免内存撑爆。
- [x] **P6-02-2** 实现 SQL 执行接口，接收原始 SQL 字符串，执行后返回 JSON 序列化好的二维表结果。
- [x] **P6-02-3** 前端开发数据库管理列表，支持查看链接状态及修改访问密码。
- [x] **P6-02-4** 前端集成一个精简版 Web SQL 查询台，左右分栏（左侧表树，右侧 Monaco SQL 输入 + 结果表格）。

---

## P7 第七步：计划任务调度 (Cron & Tasks)

目标：安全、隔离的周期性任务管理系统。

### P7-01 内部调度器 (`tokio-cron-scheduler`)
- [x] **P7-01-1** 定义 `cron.proto`，支持任务定义、暂停、恢复、一次性测试运行。
- [x] **P7-01-2** 在 Rust 后端全局初始化 `tokio-cron-scheduler` 实例，接管操作系统的定时职责。
- [x] **P7-01-3** 建立持久化存储（如 SQLite 或 JSON 文件）记录用户设定的计划任务规则，防止重启丢失。

### P7-02 隔离执行与日志审计
- [x] **P7-02-1** 编写独立的 Task Runner：收到调度信号后利用 `tokio::process::Command` 派生子进程执行 Bash 脚本。
- [x] **P7-02-2** 实现子进程的执行超时管理 (Timeout)，防止僵尸进程耗尽系统资源。
- [x] **P7-02-3** 将执行期间的 stdout/stderr 重定向到任务专属的按天滚动的日志文件中。

### P7-03 任务调度前端
- [x] **P7-03-1** 前端开发任务列表界面，直观展示每个任务的 Cron 表达式及其“下一次预计执行时间”。
- [x] **P7-03-2** 开发任务创建表单（类型支持：Shell 脚本、数据库备份、日志清理等预设模板）。
- [x] **P7-03-3** 开发“执行日志”抽屉组件，可翻阅历史运行情况和报错详情。

---

## 验证计划

- [x] **V-01** `./scripts/verify-all.sh` 脚本执行，覆盖所有前后端校验。
- [x] **V-02** 所有的 Backend Rust 单元测试通过：`cargo test --all-targets`。
- [x] **V-03** 所有的前端组件 Linter / Test 通过：`bun lint && bun test`。
- [ ] **V-04** 端对端场景演练：面板完整重启后 -> 守护进程存活 -> 一键拉起 MySQL 容器 -> 创建反代站点并自动签发证书 -> 完成流式备份计划任务。 @status: BLOCKED - 当前执行环境缺少 `nginx`，无法完成反向代理配置检测、重载和证书签发后的站点演练。
