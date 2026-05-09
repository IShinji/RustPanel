# RustPanel

RustPanel 是一个基于 Rust 后端、GHCR 镜像和 Docker Compose 部署的项目仓库。

## 常用命令

```bash
./scripts/verify-all.sh
bun run scripts:build
bash deploy/update.sh
```

## 部署约定

- 默认后端镜像：`ghcr.io/ishinji/rustpanel-backend`
- 默认 Compose 项目名：`rustpanel`
- 默认 API 端口：`18080`
- 默认 VPS 目录：`/www/wwwroot/rustpanel`

## GitHub Runner

本机 self-hosted runner 配置在 `deploy/github-runner/`，使用 `rustpanel-runner` 项目专属 label，并复用个人多项目 runner 的共享 Rust/Docker 缓存 volume。
