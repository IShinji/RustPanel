# Vibe Coding Project Template

这个模板用于快速启动新的 AI 协作项目。复制到新仓库后，替换所有 `{{...}}` 占位符，并根据项目实际模块删除不需要的部分。

## 使用步骤

1. 复制模板内容到新仓库根目录。
2. 全局替换占位符：
   - `{{PROJECT_NAME}}`: 项目显示名，例如 SuperSpider。
   - `{{PROJECT_SLUG}}`: 小写短名，例如 superspider。
   - `{{BACKEND_BINARY}}`: Rust 后端二进制名。
   - `{{GHCR_IMAGE}}`: GHCR 镜像名，例如 `ghcr.io/<owner>/<project>-backend`。
   - `{{API_PORT}}`: 宿主机 API 端口。
   - `{{DEPLOY_DIR}}`: VPS 部署目录。
   - `{{PRODUCT_DOMAIN}}`: 生产域名。
3. 执行 `git config core.hooksPath .githooks`。
4. 配置 GitHub secrets / variables。
5. 第一次提交前运行 `./scripts/verify-all.sh --all`。

## 包含内容

- Agent 协作规范：`AGENTS.md`、`CLAUDE.md`、`GEMINI.md`
- 角色、流程、版本、审查和规划模板
- GitHub Actions CI/CD 模板
- GHCR + Docker Compose + VPS updater 模板
- 根目录 TypeScript 自动化脚本编译链路
- 版本自动 patch bump、CI 状态检查、workflow guard、GHCR 清理
