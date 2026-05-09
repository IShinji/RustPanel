# RustPanel 入门指南 (Getting Started)

欢迎来到 RustPanel。这是一个旨在通过现代化技术栈（Rust + React + Docker + gRPC）完全重构服务器管理体验的新型控制面板。

本指南将帮助你快速了解项目的技术背景，并在本地开启探索与开发。

## 前置环境依赖

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

作为一名初次接触此项目的开发者，强烈建议你首先阅读：
👉 **[RustPanel 架构演进与功能闭环路线图](../planning/tasks-v2.0-architecture-evolution.md)**

该文档详细列出了实现 RustPanel 从骨架到具体业务逻辑（如 Web SSH，文件管理，Nginx 自动反代，证书自动签发等）的 7 个阶段及所有对应的 P1 级任务。

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

由于当前项目正在向“第一步：通信底座与系统守护 (Foundation & Daemon)”进发，你可以参考上述规划文档，按步骤逐步将功能落实到 `src` 和 `web` 目录。
