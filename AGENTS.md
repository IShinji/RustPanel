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
- **状态持久化**:JSON 状态文件一律 tmp+rename 原子写;同一文件的 load→改→save
  用进程内 `tokio::sync::Mutex` 串行化,防并发丢更新与半截文件。
- **机密**:私钥等敏感文件落盘后收紧到 `0600`。
