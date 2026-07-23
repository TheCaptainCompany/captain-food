# Cached Rust build for Render (ADR-0042). cargo-chef makes the dependency compilation its own Docker
# layer, which Render caches across deploys — so a redeploy that only touches app code skips recompiling
# the ~200 dependencies (the native runtime recompiled them every time). The runtime image carries just
# the server binary. Migrations are applied out-of-band by sqlx-cli in CI (ADR-0043).
FROM rust:1-bookworm AS chef
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/* \
    && cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
# The wasm toolchain (split 4/4 of #21): the hydrate bundle is built INTO the image so the server
# can serve /assets/web.js. wasm-bindgen-cli is pinned to the exact wasm-bindgen version of
# crates/web (the CLI refuses a mismatch) — bump both together.
RUN cargo install wasm-bindgen-cli --locked --version 0.2.126
COPY --from=planner /app/recipe.json recipe.json
# Cook dependencies only — cached unless Cargo.lock / a Cargo.toml changes. Two cooks: the native
# server tree and the wasm32 hydrate tree (each is its own cached layer). The wasm target comes
# with the toolchain via rust-toolchain.toml `targets` (cargo-chef's skeleton carries the file, so
# the toolchain rustup resolves AT COOK TIME is the file's — a target added in an earlier layer
# landed on the base image's default toolchain instead, which is exactly how the first image build
# broke); the explicit add here is an idempotent belt-and-braces for the active toolchain.
RUN cargo chef cook --release --recipe-path recipe.json
RUN rustup target add wasm32-unknown-unknown \
    && cargo chef cook --release --recipe-path recipe.json \
       --target wasm32-unknown-unknown --no-default-features --features hydrate --package web
COPY . .
RUN cargo build --release -p server
# The hydrate bundle: wasm32 cdylib -> wasm-bindgen --target web -> /app/dist (web.js + web_bg.wasm).
RUN cargo build --release -p web --target wasm32-unknown-unknown --no-default-features --features hydrate \
    && wasm-bindgen --target web --no-typescript --out-dir /app/dist --out-name web \
       target/wasm32-unknown-unknown/release/web.wasm

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/server /usr/local/bin/server
# The wasm hydrate bundle, served by the server under /assets (WEB_ASSETS_DIR default).
COPY --from=builder /app/dist /app/web-assets
# Precise build identity for diagnostics (ADR-20260721-175411): CI passes the exact git commit SHA. The
# ARG is declared ONLY in this final stage (after the COPY) so a new SHA changes just these trailing
# metadata layers and never invalidates the cached cargo-chef builder layers. The server reads
# $CAPTAIN_BUILD_VERSION and reports it at /health; the OCI labels make `docker inspect` show it too.
ARG CAPTAIN_BUILD_VERSION=dev
ENV CAPTAIN_BUILD_VERSION=$CAPTAIN_BUILD_VERSION
LABEL org.opencontainers.image.revision=$CAPTAIN_BUILD_VERSION \
      org.opencontainers.image.source=https://github.com/TheCaptainCompany/captain-food
# $PORT is injected by Render; the server binds 0.0.0.0:$PORT.
CMD ["server"]
