# RustPanel

RustPanel 是一款对标宝塔免费功能、但底层全面现代化的新一代 Rust 服务面板。它摒弃了传统面板混杂的 API 结构与重度依赖宿主机环境的设计，采用强类型契约、极简部署机制与“容器优先”的现代应用架构。

## 核心特性

- **现代通信底座**：采用 **Protobuf + gRPC** 和 Axum 混合路由，实现高性能、强约束的通信协议。
- **极简部署**：利用 `rust-embed` 将前端打包内联进单一二进制文件，只需一个文件即可运行整个控制面板。
- **实时监控与极速终端**：内嵌基于 `xterm.js` + `portable-pty` 的 Web SSH，并实时推流展示系统状态指标。
- **现代化文件管理**：非阻塞的深层文件遍历、大文件流式传输，以及集成 Monaco Editor 的在线源码编辑。
- **Docker-First 应用商店**：抛弃易污染宿主机的源码编译模式，直接对接 Docker API，全面容器化管理（支持 MySQL, Redis 等一键拉起与 Compose 编排）。
- **极简 Web Server 与自动 SSL**：化繁为简的 Nginx 反代配置，结合 `acme-lib` 自动化 Let's Encrypt 证书签发与后台静默续期。
- **现代化运维与安全**：v3.0 正在对齐宝塔免费版核心功能，包含可视化防火墙、进阶 Nginx 配置及历史监控。

## 架构演进路线图

关于 RustPanel 的路线图与各个阶段的详细任务：
- [v2.0 架构演进路线](docs/planning/tasks-v2.0-architecture-evolution.md)（已基本完成）
- [v3.0 宝塔免费版功能对齐](docs/planning/tasks-v3.0-free-version-alignment.md)（进行中）

## 快速开始

### 一键安装

Linux 服务器可使用类似宝塔的下载安装脚本方式部署：

```bash
url=https://raw.githubusercontent.com/IShinji/RustPanel/main/deploy/install.sh;if command -v curl >/dev/null 2>&1;then curl -fsSL "$url" -o rustpanel-install.sh;else wget -O rustpanel-install.sh "$url";fi;sudo bash rustpanel-install.sh
```

默认安装到 `/www/wwwroot/rustpanel`。脚本默认进入**交互式安装**，会依次询问：

- NAT 放通端口段（例：`1200-1219`，无 NAT 限制直接回车跳过）
- 面板访问端口（浏览器实际访问的端口，必须落在 NAT 段内）
- 公网 IP 或域名（NAT 机自动探测会拿到内网 IP，需手动覆盖）
- 绑定地址、Profile、HTTPS Origin、管理员凭据
- 是否启用极限低内存模板（micro 档可再禁 proxy/workloads）

完成后管理员密码、JWT 密钥、最终面板 URL 都会打印并写入 `/www/wwwroot/rustpanel/.env`。

非交互场景（CI、自动化）传 `--assume-recommended` 跳过所有提问：

```bash
sudo bash rustpanel-install.sh --assume-recommended --port 18888 --origin https://panel.example.com
sudo bash rustpanel-install.sh --assume-recommended --bind 127.0.0.1 --port 18080
sudo bash rustpanel-install.sh --assume-recommended --profile micro \
  --nat-port-range 1200-1219 --port 1200 --public-host 1.2.3.4
```

### Micro 极限模式

`micro` 档位面向 128MB RAM、2GB 磁盘、NAT IPv4、OpenVZ 这类极限小鸡。安装器会先做磁盘/内存硬门禁（<500MB / <80MB 直接 fail），再探测虚拟化和 Docker 能力，在低配或 OpenVZ 环境下推荐二进制裸跑模式，默认启用内置静态托管、轻量任务托管和用户态代理，禁用 Docker、应用商店、Nginx 站点和 SSL 自动化。128MB 实在紧时可在交互流程末尾启用、或直接加 `--ultra-low` 再禁掉 proxy 和 workloads。

安装前可先查看建议：

```bash
sudo bash rustpanel-install.sh --dry-run
```

请参阅完整使用指南：[入门指南 (Getting Started)](docs/guide/getting-started.md)

## 开发与协作规范

本项目采用 Agent 协作驱动（Vibe Coding）的理念推进。为保证最高优先级的质量门槛，所有 Agent 或开发者参与时必须遵循以下规范：
- `AGENTS.md`：项目的最高优先级执行规则（架构约束、验证、提交规范等）。
- `CLAUDE.md`：角色目录和执行流程约束。
- `GEMINI.md`：快捷上下文与常用指令。
