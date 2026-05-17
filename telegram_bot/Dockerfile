# Multi-stage build: compile statically-friendly bot then ship slim runtime.
FROM rust:1.83-slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /build
# Copy the workspace pieces we need
COPY client_rust ./client_rust
COPY telegram_bot ./telegram_bot

WORKDIR /build/telegram_bot
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/telegram_bot/target/release/telegram_bot /usr/local/bin/telegram_bot
ENV RUST_LOG=info
CMD ["telegram_bot"]
