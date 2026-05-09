# RustPanel Docker Compose Runner

这个目录用于在当前 M4 开发机上启动 RustPanel 专属 GitHub Actions self-hosted runner。容器是 Linux ARM64，工作区隔离在 `/runner`，Rust 和 Docker 缓存通过 Docker volume 复用。

## 首次启动

```bash
cd deploy/github-runner
cp .env.example .env
```

在 GitHub 仓库 `Settings -> Actions -> Runners -> New self-hosted runner` 复制 registration token，填入 `.env` 的 `GITHUB_RUNNER_TOKEN`。

OrbStack 默认使用：

```env
DOCKER_SOCKET=/Users/wisely/.orbstack/run/docker.sock
```

Docker Desktop 通常使用：

```env
DOCKER_SOCKET=/var/run/docker.sock
```

启动：

```bash
docker compose --env-file .env up -d --build
docker compose logs -f rustpanel-runner
```

确认 runner online 后，workflow 使用这些 labels：

```yaml
runs-on: [self-hosted, Linux, ARM64, m4-runner, rustpanel-runner]
```

如果 workflow 使用仓库变量切换 runner，RustPanel 对应值建议为：

```bash
gh variable set RUSTPANEL_CI_RUNNER_MODE --body self-hosted
gh variable set RUSTPANEL_SELF_HOSTED_RUNNER_LABELS --body '["self-hosted","Linux","ARM64","m4-runner","rustpanel-runner"]'
gh variable set RUSTPANEL_BUILDKIT_CACHE_DIR --body '/cache/buildkit/rustpanel'
```

## 日常操作

```bash
docker compose ps
docker compose logs -f rustpanel-runner
docker compose stop
docker compose start
docker compose up -d --build
```

`rustpanel-runner-data` 保存 runner 注册状态。不要执行 `docker compose down -v`，除非你明确要删除注册状态和共享缓存并重新冷启动。

如果需要修改 runner name 或 labels，先在 GitHub 页面删除旧 runner，再删除 `rustpanel-runner-data` 并使用新的 registration token 重新启动。

## 共享缓存

这些 volume 设计为跨个人项目复用：

```text
github-runner-cargo
github-runner-rustup
github-runner-sccache
github-runner-tool
github-runner-buildkit
```

RustPanel 专属数据：

```text
rustpanel-runner-data
/cache/buildkit/rustpanel
```

## 与 SuperCard 共享缓存

RustPanel runner 和 SuperCard runner 复用同一组 Docker volumes：

```text
github-runner-cargo
github-runner-rustup
github-runner-sccache
github-runner-buildkit
```

`Release CI & Deploy` 会把 RustPanel 的 BuildKit cache 导出到 `/cache/buildkit/rust-backend-linux-amd64-v1/rustpanel/backend-image`，同时在构建时读取同一 family 下 SuperCard 的 `/cache/buildkit/rust-backend-linux-amd64-v1/supercard/backend-image`。self-hosted runner 上本地 BuildKit cache 是权威路径，不再同时导入或导出 GHCR registry buildcache。Dockerfile 内部使用与 SuperCard 相同的 Cargo cache mount id：

```text
rust-cargo-registry-v1
rust-cargo-git-v1
rust-sccache-v1
```

这样可以共享 crates.io registry、git 依赖缓存、sccache 编译结果和可复用的 BuildKit 层。`target` 目录 cache 仍保持 RustPanel 专属，避免两个不同 workspace 的增量产物互相污染或无限膨胀。

多项目接入细节参考 `/Users/wisely/Documents/GitHub/SuperCard/docs/guides/personal-multi-project-runner.md`。
