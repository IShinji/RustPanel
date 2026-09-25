# AGENTS.md

本文档是 `RustPanel` 的最高优先级执行规则。

## Source Of Truth

- `AGENTS.md`: 仓库规则、自动执行、停止条件、验证、提交、推送、架构约束。
- `CLAUDE.md`: 角色目录、角色选择和执行流程。
- `GEMINI.md`: 快捷上下文和命令速查。

冲突优先级：`AGENTS.md` → `CLAUDE.md` → `GEMINI.md`。

## 默认执行

- 直接用户请求：完成请求范围、验证、提交、推送，并等待最终 CI / Deploy 通过。
- 任务清单模式：按 `docs/planning/tasks-v*.md` 的最高优先级继续执行，除非任务明确 BLOCKED。
- 禁止询问“是否继续”；只有产品范围不清、破坏性变更、安全隐私风险、不可逆操作或连续失败 3 次时才暂停。

## 质量门槛

- Backend: `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test --all-targets`。
- Web/Admin: `bun lint`、`bun test`、`bun run build`。
- Unified: `./scripts/verify-all.sh`。
- Terminal CI Gate: `bun run scripts:build && node dist/node-scripts/scripts/check-latest-ci.js --commit <sha> --wait`。

## 提交规范

- 使用 Conventional Commits。
- Commit title 使用英文 type + 中文主题。
- Commit body 使用中文 changelog 风格。
- 多行 commit message 必须使用 `/tmp/rustpanel-commit-message.txt` 和 `git commit -F`。

## 部署规范

- Docker/GHCR/Compose 资源必须使用 `rustpanel-*` 命名。
- VPS 目录默认 `/www/wwwroot/rustpanel`。
- GHCR image 默认 `ghcr.io/ishinji/rustpanel-backend`。

## 架构约束

- **目标环境是低配 / 受限主机**:NAT VPS、OpenVZ、低至 ~128MB 内存、IPv6 多 IP。
  「装得下、跑得动」优先于「功能多」。每个新功能都按此设计:
  - **内存**:不要把大对象整体读进内存。文件 / 备份 / 上传下载一律**流式**处理
    (tar/gz 流式、reqwest 流式 body + `bytes_stream`、分片);禁止 `fs::read` 整文件到 `Vec`
    再发送 / 落盘。
  - **进程 / fork**:谨慎起子进程;能用纯 Rust(rcgen / tar / flate2)就别 shell out。
    重型运行时(nginx/MySQL/PHP)优先容器化或换轻量替代(rpxy / sws / sqlite)。
  - **依赖**:新增 crate 权衡体积与编译成本,默认 `default-features = false` + rustls,
    不引 openssl / native-tls。
  - **后台任务**:周期间隔保守(分钟级),扫描廉价,无消费者时直接跳过。
  - 尊重 CapabilityService 能力探针与资源预算(min_ram / NAT 端口预算)。
  - **运行形态分层(关键)**:OpenVZ / 极低配大概率**跑不了 Docker**(无 overlayfs、
    user_namespaces 受限、daemon 本身吃几十~上百 MB);`CapabilityService` 探测
    `can_run_docker` 并让前端置灰 Docker / AppStore。低配主线必须是**非容器**路径:
    静态站(sws)+ 反代(rpxy)+ `WorkloadService` 跑原生二进制/脚本。容器化(含容器
    PHP-FPM)是**能跑 Docker 主机**(KVM + 够内存)的增强,**不能当作低配场景的唯一方案**;
    动态 PHP 等重型站点在 ~128MB 上不在目标内。凡"用容器解决"的功能,必须确认
    `can_run_docker=false` 时有非容器后备或已明确置灰,不留"只在能跑 Docker 时才可用"的隐性缺口。
- **节俭模式(micro 默认)**:面板由 `rustpanel-backend.socket` 按需唤醒,空闲
  `RUSTPANEL_IDLE_EXIT_MINUTES`(默认 10)分钟后 exit 0。凡是**请求返回后仍在跑**的
  工作(WS 会话、`tokio::spawn` 的部署/回滚计时器、托管子进程)都必须持有
  `crate::frugal::BusyGuard`,否则会被空闲退出打断;普通请求与流式响应已由多路复用层自动计入。
  周期性工作不要再做进程内调度器:计划任务走系统 cron(`--run-cron-task`),
  证书续签走 `rustpanel-cert-renew.timer`(`--renew-certs`)。
- **状态持久化**:JSON 状态文件一律 tmp+rename 原子写;同一文件的 load→改→save
  用进程内 `tokio::sync::Mutex` 串行化,防并发丢更新与半截文件。
  **不要再手写 tmp+rename**,统一走 `crate::statefile::write_atomic`。
- **机密**:私钥等敏感文件落盘后收紧到 `0600`。含凭据的状态文件(备份去向、通知渠道、
  用户库、ACME pending、集群 node_secret 等)一律用 `crate::statefile::write_secret_atomic`
  —— 它在 rename **之前**把权限打到 tmp 上,目标路径不留 0644 窗口。
- **认证**:JWT 自包含,改角色 / 改密码 / 删用户必须同时记一次吊销水位
  (`crate::user::revoke_tokens_in`),否则旧 token 在 TTL 内照样全权有效。
  新增对外认证入口时,先过限速再做口令校验(PBKDF2 十万轮在低配机上就是 CPU DoS 面)。
- **单响应体积**:任何「把整个文件/整张表塞进一个 gRPC 响应」的接口都必须自己设上限
  —— tonic 出站默认不限大小。大对象走已有的分片上传 / 流式下载通道。
