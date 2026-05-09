# syntax=docker/dockerfile:1.7
FROM rust:1-bookworm AS ci-checks
WORKDIR /app
RUN rustup component add rustfmt clippy
RUN --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=rustpanel-cargo-install-target-v1,target=/app/target,sharing=locked \
    cargo install sccache --version 0.15.0 --locked
ENV RUSTC_WRAPPER=sccache
ENV SCCACHE_DIR=/sccache
ENV SCCACHE_CACHE_SIZE=50G
ENV CARGO_INCREMENTAL=0
COPY . .
WORKDIR /app/src/backend
RUN cargo fmt --check
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=rustpanel-backend-ci-target-v1,target=/app/target,sharing=locked \
    cargo clippy --locked --all-targets -- -D warnings
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    --mount=type=cache,id=rust-cargo-registry-v1,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rust-cargo-git-v1,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=rustpanel-backend-ci-target-v1,target=/app/target,sharing=locked \
    cargo test --locked --all-targets -- --quiet
RUN --mount=type=cache,id=rust-sccache-v1,target=/sccache,sharing=locked \
    sccache --show-stats || true

FROM ci-checks AS builder
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
