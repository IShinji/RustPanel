# syntax=docker/dockerfile:1.7
FROM rust:1-bookworm AS ci-checks
WORKDIR /app
COPY . .
WORKDIR /app/src/backend
RUN rustup component add rustfmt clippy
RUN cargo fmt --check
RUN cargo clippy --locked --all-targets -- -D warnings
RUN cargo test --locked --all-targets -- --quiet

FROM ci-checks AS builder
RUN cargo build --release --locked --bins

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/src/backend/target/release/rustpanel-backend /app/rustpanel-backend
ENV RUSTPANEL_BACKEND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["/app/rustpanel-backend"]
