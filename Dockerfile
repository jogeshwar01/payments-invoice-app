# Multi-stage build: one image, two binaries (dodo-api, mock-psp).
FROM rust:1.90-slim-bookworm AS builder
WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Cache deps first.
COPY Cargo.toml Cargo.lock* ./
COPY crates/api/Cargo.toml crates/api/Cargo.toml
COPY crates/mock-psp/Cargo.toml crates/mock-psp/Cargo.toml
RUN mkdir -p crates/api/src crates/mock-psp/src \
    && echo "fn main() {}" > crates/api/src/main.rs \
    && echo "pub fn _x() {}" > crates/api/src/lib.rs \
    && echo "fn main() {}" > crates/mock-psp/src/main.rs \
    && cargo build --release \
    && rm -rf crates/api/src crates/mock-psp/src

COPY crates ./crates
COPY migrations ./migrations
RUN touch crates/api/src/main.rs crates/api/src/lib.rs crates/mock-psp/src/main.rs \
    && cargo build --release --bin dodo-api --bin mock-psp

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/dodo-api /usr/local/bin/dodo-api
COPY --from=builder /build/target/release/mock-psp /usr/local/bin/mock-psp
COPY --from=builder /build/migrations /app/migrations
WORKDIR /app
