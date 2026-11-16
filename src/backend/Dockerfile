# syntax=docker/dockerfile:1.29

# 所有编译阶段都钉在 BUILDPLATFORM(原生 arm64 runner)上,靠 zig 交叉编译出 x86_64 产物。
# 原因:runner 只有 ARM64,而 QEMU 模拟出来的 rustc 会直接 SIGSEGV
# (日志里是 sccache 报 "qemu: uncaught target signal 11"),--platform linux/amd64
# 的老路子已经编译不出来任何东西了;web 侧同理,Bun 在模拟环境里会 MemoryExhaustion abort。
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS chef
ARG ZIG_VERSION=0.17.1
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl xz-utils \
    && rm -rf /var/lib/apt/lists/*
RUN --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo install sccache --version 0.15.0 --locked \
    && cargo install cargo-chef --locked \
    && cargo install cargo-zigbuild --locked
# zig 只用来当 x86_64 的链接器/CC(musl 与 glibc 都覆盖),不在 guest 里跑任何 emulation。
RUN set -eux; \
    arch="$(uname -m)"; \
    curl -fsSL "https://ziglang.org/download/${ZIG_VERSION}/zig-${arch}-linux-${ZIG_VERSION}.tar.xz" \
      | tar -xJ -C /opt; \
    ln -s "/opt/zig-${arch}-linux-${ZIG_VERSION}/zig" /usr/local/bin/zig; \
    zig version
RUN rustup target add x86_64-unknown-linux-gnu x86_64-unknown-linux-musl

# sccache 只给原生(ci-checks)链路用:交叉编译阶段带上它会让 cc-rs 把 zig 也套一层
# sccache,间接把 C 编译拆坏("sccache zig cc -target ..." → zig: unknown command -E)。
ENV SCCACHE_DIR=/sccache
ENV SCCACHE_CACHE_SIZE=50G
ENV CARGO_INCREMENTAL=0
ENV PROTOC=/usr/bin/protoc

# 静态站点产物(dist)与目标架构无关,固定跑在 BUILDPLATFORM 上:
# 跟着 --platform linux/amd64 走 QEMU 时 Bun 1.4.4 会以 "MemoryExhaustion" 直接 abort
# (bun run build 退出码 134),把三个发布 job 全带挂。
FROM --platform=$BUILDPLATFORM oven/bun:1.4.4 AS web-builder
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

# CI 自检用:原生(BUILDPLATFORM)跑 fmt/clippy/test,不碰交叉编译链路。
FROM chef AS ci-deps
ENV RUSTC_WRAPPER=sccache
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

# x86_64 glibc 产物(Debian/Ubuntu 主机用)。
# micro 二进制:--build-arg BACKEND_CARGO_FLAGS=--no-default-features
# (去掉 bollard / MySQL / Postgres / Redis / portable-pty,见 src/backend/Cargo.toml)
# 注意:这里刻意不用 cargo chef cook —— cook 走的是普通 cargo,cc-rs 会把 Rust triple
# 当成 "--target=x86_64-unknown-linux-gnu" 塞给 zig,zig 只认自己的 triple 会直接报错。
# cargo-zigbuild 自带正确的 CC/AR/linker(ring 之类的 C/汇编 crate 也能编),代价是
# 依赖层不再被 chef 缓存,换源文件会重编依赖(原生编译,比原来 QEMU 模拟快得多)。
FROM chef AS builder
ARG BACKEND_CARGO_FLAGS=""
ARG TARGET_TRIPLE=x86_64-unknown-linux-gnu
COPY . .
COPY --from=web-builder /app/src/web/dist /app/src/web/dist
WORKDIR /app/src/backend
RUN --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo zigbuild --release --locked --bins --target $TARGET_TRIPLE $BACKEND_CARGO_FLAGS

# 只导出二进制(--target binary-export);必须排在运行时镜像之前 ——
# 不指定 target 时 docker 用最后一个阶段,排在最后会让推送的镜像变成没有 CMD 的空壳。
FROM scratch AS binary-export
COPY --from=builder /app/target/x86_64-unknown-linux-gnu/release/rustpanel-backend /rustpanel-backend

# x86_64 musl 静态产物(Alpine/OpenRC 主机用),同样走 BUILDPLATFORM 交叉编译。
# 只构建 micro 档(--no-default-features)——128MB 级 Alpine 小鸡这次唯一的目标场景;
# full 档在 Alpine 上没有实际需求,不在这里陪跑一份浪费 CI 时间。
FROM chef AS builder-musl
ARG BACKEND_CARGO_FLAGS="--no-default-features"
ARG TARGET_TRIPLE=x86_64-unknown-linux-musl
COPY . .
COPY --from=web-builder /app/src/web/dist /app/src/web/dist
WORKDIR /app/src/backend
RUN --mount=type=cache,id=rust-musl-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-musl-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo zigbuild --release --locked --bins --target $TARGET_TRIPLE $BACKEND_CARGO_FLAGS

FROM scratch AS binary-export-musl
COPY --from=builder-musl /app/target/x86_64-unknown-linux-musl/release/rustpanel-backend /rustpanel-backend

# 运行时镜像只 COPY 交叉编译好的 x86_64 二进制,不在 guest 里执行任何东西,
# 所以基础镜像拉 linux/amd64 也不会触发 emulation。
FROM debian:bookworm-slim AS runtime
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-gnu/release/rustpanel-backend /app/rustpanel-backend
ENV RUSTPANEL_BACKEND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["/app/rustpanel-backend"]
