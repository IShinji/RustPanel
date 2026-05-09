# RustPanel 入门指南 (Getting Started)

欢迎来到 RustPanel。这是一个旨在通过现代化技术栈（Rust + React + Docker + gRPC）完全重构服务器管理体验的新型控制面板。

本指南将帮助你快速了解项目的技术背景，并在本地开启探索与开发。

## 前置环境依赖

如果只是部署到 Linux 服务器，可直接使用一键安装脚本，脚本会自动准备 Docker 并启动 GHCR 镜像：

```bash
url=https://raw.githubusercontent.com/IShinji/RustPanel/main/deploy/install.sh;if command -v curl >/dev/null 2>&1;then curl -fsSL "$url" -o rustpanel-install.sh;else wget -O rustpanel-install.sh "$url";fi;sudo bash rustpanel-install.sh
```

常见参数：

```bash
sudo bash rustpanel-install.sh --port 18888 --origin https://panel.example.com
sudo bash rustpanel-install.sh --bind 127.0.0.1 --skip-docker-install
```

默认安装目录是 `/www/wwwroot/rustpanel`，配置文件是 `/www/wwwroot/rustpanel/.env`，后续升级可执行：

```bash
sudo bash /www/wwwroot/rustpanel/deploy/update.sh
```

以下依赖仅用于本地开发：

开始之前，请确保你的开发环境安装了以下工具：

1. **Rust 工具链**: 用于编译后端核心与生成二进制文件。
   - 安装命令: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
   - 要求: 1.80+ (推荐使用稳定版)
2. **Node.js 与 Bun**: 用于前端界面的构建及相关脚本运行。
   - 建议使用 Bun 作为包管理器: `npm install -g bun`
3. **Protobuf 编译器 (`protoc`)**: 用于将 `.proto` 文件编译为 Rust 和 TypeScript 代码。
   - macOS: `brew install protobuf`
   - Linux: `sudo apt install protobuf-compiler`
4. **Docker**: 用于本地联调应用商店功能与容器引擎。

## 目录结构速览

```text
RustPanel/
├── docs/                # 文档存放区
│   ├── planning/        # 架构规划与任务清单
│   └── guide/           # 使用指南与开发手册
├── proto/               # gRPC 通信协议定义目录 (Single Source of Truth)
├── src/                 # 后端 Rust 代码
├── web/                 # 前端 React / Vite 代码 (待初始化)
├── scripts/             # 自动化脚本与验证工具
└── README.md            # 项目入口文档
```

## 开始阅读架构规划

作为一名初次接触此项目的开发者，建议你阅读最新的路线图：
- 👉 **[v3.0 宝塔免费版功能对齐](../planning/tasks-v3.0-free-version-alignment.md)** (当前正在进行的阶段)
- 👉 **[v2.0 架构演进与功能闭环](../planning/tasks-v2.0-architecture-evolution.md)** (了解系统底层架构)

## 项目协作（面向 AI 与开发者）

本项目通过一系列顶层 Markdown 文件定义了严格的协作规范：
- **`AGENTS.md`**: 定义质量门槛、默认执行模式以及提交流程（例如使用 Conventional Commits 提交代码）。
- **`GEMINI.md`**: 快速复习常用构建和校验命令。
- **`CLAUDE.md`**: 若使用 Agent 参与，可参考此文档中定义的专家身份（如系统级 Rust 开发、前端交互设计）。

## 验证与构建

为了保证代码库的健康，所有改动提交前，必须运行以下命令进行全局验证：

```bash
# 执行全套校验（包含后端 fmt、clippy、test 和前端 lint/test）
./scripts/verify-all.sh
```

目前项目已完成核心骨架构建，正在逐步补齐高级运维功能。如果你是第一次运行，建议从 v3.0 的“安全底座”任务开始。
