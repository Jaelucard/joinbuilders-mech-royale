# Multi-stage Rust build for linux/amd64 mechfight/1 runtime
FROM rust:1.85-slim-bookworm AS builder
WORKDIR /build

# Cache dependencies first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release --locked && \
    rm -rf src

# Build the real binary
COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

FROM debian:bookworm-slim
WORKDIR /bot
COPY --from=builder /build/target/release/mechbot /bot/mechbot
ENTRYPOINT ["/bot/mechbot"]
