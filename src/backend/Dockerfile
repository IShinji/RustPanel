# syntax=docker/dockerfile:1.7

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
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
WORKDIR /app/src/backend
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    cargo build --release --locked --bins
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    sccache --show-stats || true

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/rustpanel-backend /app/rustpanel-backend
ENV RUSTPANEL_BACKEND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["/app/rustpanel-backend"]
