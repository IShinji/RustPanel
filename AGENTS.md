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
