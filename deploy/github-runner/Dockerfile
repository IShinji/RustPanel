FROM ubuntu:24.04

ARG RUNNER_VERSION=2.334.0
ARG TARGETARCH

ENV DEBIAN_FRONTEND=noninteractive
ENV RUNNER_ALLOW_RUNASROOT=1
ENV BUN_INSTALL=/usr/local/bun
ENV CARGO_HOME=/cache/cargo
ENV RUSTUP_HOME=/cache/rustup
ENV PATH=/usr/local/bun/bin:/cache/cargo/bin:${PATH}

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        docker.io \
        git \
        jq \
        libssl-dev \
        pkg-config \
        sudo \
        tar \
        unzip \
        xz-utils \
        build-essential \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /opt/actions-runner /runner /cache/cargo /cache/rustup /cache/sccache /cache/buildkit /cache/tool

WORKDIR /opt/actions-runner

RUN case "${TARGETARCH}" in \
        arm64) runner_arch=arm64 ;; \
        amd64) runner_arch=x64 ;; \
        *) echo "Unsupported TARGETARCH=${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && curl -fsSL -o actions-runner.tar.gz \
        "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-${runner_arch}-${RUNNER_VERSION}.tar.gz" \
    && tar xzf actions-runner.tar.gz \
    && rm actions-runner.tar.gz \
    && ./bin/installdependencies.sh

RUN curl -fsSL https://bun.sh/install | bash \
    && curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
    && cargo install cargo-sweep --locked \
    && cargo install sccache --version 0.15.0 --locked

COPY entrypoint.sh /usr/local/bin/github-runner-entrypoint.sh
RUN chmod +x /usr/local/bin/github-runner-entrypoint.sh

WORKDIR /runner

ENTRYPOINT ["/usr/local/bin/github-runner-entrypoint.sh"]
