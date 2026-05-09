# CLAUDE.md - RustPanel 开发 Agent 配置

## 当前上下文

- 项目：`RustPanel`
- 部署：GHCR + Docker Compose + Cloudflare Pages / 静态托管
- 默认后端镜像：`ghcr.io/ishinji/rustpanel-backend`

## 流程

### 直接用户请求

1. 同步远端并确认工作区状态。
2. 读取相关规格、角色和代码。
3. 实现最小安全变更。
4. 按变更范围验证。
5. 使用 message file 提交并推送。
6. 等待最终 CI / Deploy 通过。
7. 输出简洁总结。

### 任务清单模式

1. 读取 `docs/planning/tasks-v*.md`。
2. 按 `blocker → high → medium → low` 选择任务。
3. 声明角色。
4. 实现、验证、更新任务状态。
5. 提交、推送、检查 CI。
6. 继续下一个未完成且未 BLOCKED 的任务。

## 角色

- DevOps: CI/CD、Docker、部署。
- Backend: API、数据库、服务层。
- Frontend: Web/Admin UI。
- QA: 测试、验收、回归。
- PM: 需求、任务拆解、规划。
