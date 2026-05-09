# GEMINI.md - RustPanel 快捷上下文

## 项目概况

`RustPanel` 是一个可由 AI agent 快速协作开发的产品仓库。

## 常用命令

```bash
git pull --rebase
git status --short
./scripts/verify-all.sh
./scripts/verify-all.sh --all
bun run scripts:build && node dist/node-scripts/scripts/check-latest-ci.js --commit <sha> --wait
```

## 目录

```text
RustPanel/
├── docs/
├── deploy/
├── scripts/
├── src/
├── AGENTS.md
├── CLAUDE.md
└── GEMINI.md
```
