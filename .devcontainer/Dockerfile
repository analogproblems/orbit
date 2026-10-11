# Orbit's development box: Ubuntu with the libraries Tauri 2 needs on Linux,
# Node 22, stable Rust with rustfmt and Clippy, Git and the GitHub CLI.
# It is meant to hold everything `npm run check` needs.
FROM node:22-bookworm-slim AS node

FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

RUN apt-get update \
    && export DEBIAN_FRONTEND=noninteractive \
    && apt-get install -y --no-install-recommends \
        build-essential \
        curl \
        file \
        gh \
        libayatana-appindicator3-dev \
        librsvg2-dev \
        libssl-dev \
        libwebkit2gtk-4.1-dev \
        libxdo-dev \
        pkg-config \
        wget \
    && rm -rf /var/lib/apt/lists/*

COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx

# Rust lives in shared folders so the container's user can add crates and
# toolchains without root.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl -fsSL -o /tmp/rustup-init \
        "https://static.rust-lang.org/rustup/dist/$(uname -m)-unknown-linux-gnu/rustup-init" \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --no-modify-path --profile minimal \
        --default-toolchain stable --component rustfmt,clippy \
    && rm /tmp/rustup-init \
    && chmod -R a+w "$RUSTUP_HOME" "$CARGO_HOME"
