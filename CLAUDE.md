# CLAUDE.md - {{PROJECT_NAME}} 开发 Agent 配置

## 当前上下文

- 项目：`{{PROJECT_NAME}}`
- 部署：GHCR + Docker Compose + Cloudflare Pages / 静态托管
- 默认后端镜像：`{{GHCR_IMAGE}}`
- 本文件定义角色目录、角色选择、直接请求流程、任务清单流程和 review 流程；执行政策以 `AGENTS.md` 为准。

## Source Of Truth

- `AGENTS.md`：仓库规则、自动执行、停止条件、验证、提交、推送和最终 CI 门禁。
- `CLAUDE.md`：角色目录、角色切换、流程形状。
- `GEMINI.md`：项目概览、命令速查、目录速查。

冲突时按 `AGENTS.md` → `CLAUDE.md` → `GEMINI.md` 处理。

## 直接用户请求流程

1. 同步远端并确认工作区状态。
2. 只读取与请求相关的文档、角色和代码片段。
3. 声明当前角色：`Current Role: <role> | Role file: <path> | Task: <title> | Reason: <reason>`。
4. 实现最小安全变更，不扩展到无关规划任务。
5. 按变更范围验证；代码变更必须运行 `./scripts/verify-all.sh`。
6. 使用 message file 提交并推送。
7. 等待最终 pushed commit 的相关 CI / Deploy 通过。
8. 输出简洁总结。

## 任务清单执行流程

仅当用户明确要求执行 `docs/planning/tasks-v*.md` 时启用：

1. 读取当前任务清单，识别版本目标、执行规则、阶段、任务、验证计划和后续拓展池。
2. 按阶段顺序和 `blocker → high → medium → low` 选择最高优先级的未完成且未 BLOCKED 任务。
3. 按任务类型选择角色，并读取对应 `docs/roles/*.md` 的必要片段。
4. 实现最小安全变更，添加或更新必要测试。
5. 运行验证，更新任务状态。
6. 提交、推送、检查 CI。
7. 若还有未完成且未 BLOCKED 的任务，自动继续。
8. 若全部完成或 BLOCKED，切换 PM 角色规划下一版本。

## Review 流程

当用户说“审查代码”或明确请求 review：

1. 默认审查当前 diff；用户指定范围时以用户范围为准。
2. 按审查范围读取对应角色文件。
3. 发现先行：优先列 bug、回归、安全、数据一致性、性能和缺失测试。
4. 需要修复时按角色切换并执行最小安全改动。
5. 复验后提交、推送并等待 CI。

## 角色宣言

首次进入任务或切换角色时输出一行：

```text
Current Role: <role> | Role file: <path> | Task: <id/title> | Reason: <short reason>
```

同一角色下不要为每次搜索、阅读、编辑或验证重复宣言。

## 角色速查表

| 编号 | 角色 | 文件 | 触发场景 |
|------|------|------|----------|
| 01 | 系统架构师 | `docs/roles/01-system-architect.md` | 架构设计、技术选型、跨模块边界 |
| 02 | 安全工程师 | `docs/roles/02-security-engineer.md` | 加密、安全、合规风险 |
| 03 | 后端工程师 | `docs/roles/03-backend-engineer.md` | API、数据库、服务端 |
| 04 | iOS 工程师 | `docs/roles/04-ios-engineer.md` | Swift/SwiftUI 开发；仅当前工作树存在 iOS 代码时执行代码变更 |
| 05 | Android 工程师 | `docs/roles/05-android-engineer.md` | Kotlin/Compose 开发；仅当前工作树存在 Android 代码时执行代码变更 |
| 06 | macOS 工程师 | `docs/roles/06-macos-engineer.md` | macOS 桌面端 |
| 07 | Windows 工程师 | `docs/roles/07-windows-engineer.md` | Windows 桌面端 |
| 08 | 前端工程师 | `docs/roles/08-frontend-engineer.md` | React/Web/Admin UI |
| 09 | DevOps 工程师 | `docs/roles/09-devops-engineer.md` | CI/CD、Docker、部署、可观测性 |
| 10 | QA 工程师 | `docs/roles/10-qa-engineer.md` | 测试、验收、回归 |
| 11 | 产品经理 | `docs/roles/11-product-manager.md` | 需求澄清、任务选择、规划 |
| 12 | UI/UX 设计师 | `docs/roles/12-ux-ui-designer.md` | 界面规范、交互设计 |
| 13 | 国际化专家 | `docs/roles/13-i18n-specialist.md` | 多语言、文案长度、locale |
| 14 | 合规顾问 | `docs/roles/14-compliance-advisor.md` | 法规遵从、隐私、数据政策 |
| 15 | 数据采集专员 | `docs/roles/15-data-acquisition-specialist.md` | 公开数据采集、结构化、来源记录 |

## 任务类型映射

```text
Web/Admin 前端任务  → 08-frontend-engineer
后端/API 任务       → 03-backend-engineer
部署/CI 任务        → 09-devops-engineer
架构设计任务        → 01-system-architect
加密/安全任务       → 02-security-engineer
测试/验收任务       → 10-qa-engineer
规划/优先级任务     → 11-product-manager
UI/交互任务         → 12-ux-ui-designer
国际化任务          → 13-i18n-specialist
合规/隐私任务       → 14-compliance-advisor
数据采集任务        → 15-data-acquisition-specialist
iOS/Android/macOS/Windows 客户端任务 → 对应客户端角色
```

## 角色文件适配要求

`docs/roles/*.md` 是可复制模板。新项目初始化后必须：

- 将 `{{PROJECT_NAME}}`、`{{PROJECT_SLUG}}` 等占位符替换为真实项目名。
- 删除项目不存在的平台角色或保留但注明“仅当目录存在时启用”。
- 将行业描述、合规要求、数据采集边界改成当前项目真实领域。
- 保持 Work Standards、语言要求、提交要求与 `AGENTS.md` 一致。
