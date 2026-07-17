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
COPY --from=planner /app/recipe.json recipe.json
# Cook dependencies only — cached unless Cargo.lock / a Cargo.toml changes.
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release -p server

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/server /usr/local/bin/server
# $PORT is injected by Render; the server binds 0.0.0.0:$PORT.
CMD ["server"]
