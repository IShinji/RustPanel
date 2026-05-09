# RustPanel 研发流水线

## 执行顺序

1. 明确请求或任务来源。
2. 读取相关规格、角色和代码。
3. 做最小安全改动。
4. 运行针对性检查。
5. 运行 `./scripts/verify-all.sh`。
6. 使用 message file 提交。
7. 推送后执行 `bun run scripts:build && node dist/node-scripts/scripts/check-latest-ci.js --commit <sha> --wait`。

## Git Hooks

```bash
git config core.hooksPath .githooks
```
