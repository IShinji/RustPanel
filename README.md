# RustPanel

RustPanel 是一款对标宝塔免费功能、但底层全面现代化的新一代 Rust 服务面板。它摒弃了传统面板混杂的 API 结构与重度依赖宿主机环境的设计，采用强类型契约、极简部署机制与“容器优先”的现代应用架构。

## 核心特性

- **现代通信底座**：采用 **Protobuf + gRPC** 和 Axum 混合路由，实现高性能、强约束的通信协议。
- **极简部署**：利用 `rust-embed` 将前端打包内联进单一二进制文件，只需一个文件即可运行整个控制面板。
- **实时监控与极速终端**：内嵌基于 `xterm.js` + `portable-pty` 的 Web SSH，并实时推流展示系统状态指标。
- **现代化文件管理**：非阻塞的深层文件遍历、大文件流式传输，以及集成 Monaco Editor 的在线源码编辑。
- **Docker-First 应用商店**：抛弃易污染宿主机的源码编译模式，直接对接 Docker API，全面容器化管理（支持 MySQL, Redis 等一键拉起与 Compose 编排）。
- **极简 Web Server 与自动 SSL**：化繁为简的 Nginx 反代配置，结合 `acme-lib` 自动化 Let's Encrypt 证书签发与后台静默续期。
- **高性能任务与数据库管控**：进程隔离的定时计划任务执行引擎，以及直连 `sqlx` 的数据库管控与零内存耗用的流式 SQL 导出。

## 架构演进路线图

关于 RustPanel v2.0 的全面架构演进和各功能闭环阶段（如 Foundation & Daemon, Web Server & Proxy 等），详见文档：[RustPanel 架构演进路线](docs/planning/tasks-v2.0-architecture-evolution.md)

## 快速开始

请参阅完整使用指南：[入门指南 (Getting Started)](docs/guide/getting-started.md)

## 开发与协作规范

本项目采用 Agent 协作驱动（Vibe Coding）的理念推进。为保证最高优先级的质量门槛，所有 Agent 或开发者参与时必须遵循以下规范：
- `AGENTS.md`：项目的最高优先级执行规则（架构约束、验证、提交规范等）。
- `CLAUDE.md`：角色目录和执行流程约束。
- `GEMINI.md`：快捷上下文与常用指令。
