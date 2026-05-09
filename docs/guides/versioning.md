# RustPanel 版本信息规范

运行产物相关变更默认 patch bump。

## 来源

- Web/Admin: `package.json`
- Backend: `Cargo.toml` / `Cargo.lock`
- Docker: CI 注入 `VERSION`、`GIT_COMMIT`、`BUILD_TIME`

## 命令

```bash
node dist/node-scripts/scripts/sync-release-version.js --check
node dist/node-scripts/scripts/build-version.js --app backend
```
