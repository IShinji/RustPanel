# syntax=docker/dockerfile:1.27

FROM rust:1-bookworm AS chef
WORKDIR /app
RUN --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo install sccache --version 0.15.0 --locked \
    && cargo install cargo-chef --locked

ENV RUSTC_WRAPPER=sccache
ENV SCCACHE_DIR=/sccache
ENV SCCACHE_CACHE_SIZE=50G
ENV CARGO_INCREMENTAL=0

# Alpine 的 musl release 构建:给 Alpine/OpenRC 主机用,跟上面的 glibc 链路
# (chef/planner/builder)完全并行、互不影响,只在产出静态 musl 二进制这一件事
# 上重合。只构建 micro 档(--no-default-features)——128MB 级 Alpine 小鸡这次
# 唯一的目标场景就是 micro,不含 Docker API/MySQL/Postgres/Redis 这些重依赖的
# full 档没有实际需求,不在这里陪跑一份浪费 CI 时间。
#
# 不装 sccache:它自己依赖 openssl-sys,Alpine 的 openssl-dev 只给动态库,
# musl 静态链接要的 libssl.a/libcrypto.a 没有现成的,折腾这个纯缓存优化不值得——
# 这条链路靠 BuildKit 的 cargo registry/git 缓存挂载就够用。
FROM rust:1-alpine AS chef-musl
WORKDIR /app
RUN apk add --no-cache musl-dev protobuf-dev
RUN --mount=type=cache,id=rust-musl-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-musl-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo install cargo-chef --locked

ENV CARGO_INCREMENTAL=0
ENV PROTOC=/usr/bin/protoc

FROM oven/bun:1.4.2 AS web-builder
WORKDIR /app/src/web
COPY src/web/package.json src/web/bun.lock ./
RUN --mount=type=cache,id=bun-web-install-v1,target=/root/.bun/install/cache,sharing=locked \
    bun install --frozen-lockfile
COPY src/web ./
RUN bun run build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY proto ./proto
COPY src/backend ./src/backend
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS ci-deps
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --locked --all-targets --recipe-path recipe.json

FROM ci-deps AS ci-checks
COPY . .
COPY --from=web-builder /app/src/web/dist /app/src/web/dist
WORKDIR /app/src/backend
RUN cargo fmt --check
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=rustpanel-backend-ci-target-v2,target=/app/target,from=ci-deps,source=/app/target,sharing=locked \
    cargo clippy --locked --all-targets -- -D warnings
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=rustpanel-backend-ci-target-v2,target=/app/target,from=ci-deps,source=/app/target,sharing=locked \
    cargo test --locked --all-targets -- --quiet
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    sccache --show-stats || true

FROM chef AS builder
# micro 二进制:--build-arg BACKEND_CARGO_FLAGS=--no-default-features
# (去掉 bollard / MySQL / Postgres / Redis / portable-pty,见 src/backend/Cargo.toml)
ARG BACKEND_CARGO_FLAGS=""
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --locked --recipe-path recipe.json $BACKEND_CARGO_FLAGS
COPY . .
COPY --from=web-builder /app/src/web/dist /app/src/web/dist
WORKDIR /app/src/backend
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo build --release --locked --bins $BACKEND_CARGO_FLAGS
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    sccache --show-stats || true

# 只导出二进制(--target binary-export);必须排在运行时镜像之前 ——
# 不指定 target 时 docker 用最后一个阶段,排在最后会让推送的镜像变成没有 CMD 的空壳。
FROM scratch AS binary-export
COPY --from=builder /app/target/release/rustpanel-backend /rustpanel-backend

FROM chef-musl AS planner-musl
COPY Cargo.toml Cargo.lock ./
COPY proto ./proto
COPY src/backend ./src/backend
RUN cargo chef prepare --recipe-path recipe.json

FROM chef-musl AS builder-musl
ARG BACKEND_CARGO_FLAGS="--no-default-features"
COPY --from=planner-musl /app/recipe.json recipe.json
RUN --mount=type=cache,id=rust-musl-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-musl-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --locked --recipe-path recipe.json $BACKEND_CARGO_FLAGS
COPY . .
COPY --from=web-builder /app/src/web/dist /app/src/web/dist
WORKDIR /app/src/backend
RUN --mount=type=cache,id=rust-musl-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-musl-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo build --release --locked --bins $BACKEND_CARGO_FLAGS

FROM scratch AS binary-export-musl
COPY --from=builder-musl /app/target/release/rustpanel-backend /rustpanel-backend

FROM debian:bookworm-slim AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/rustpanel-backend /app/rustpanel-backend
ENV RUSTPANEL_BACKEND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["/app/rustpanel-backend"]
