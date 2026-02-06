# syntax=docker/dockerfile:1.6
FROM rust:1.88-bullseye AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock rust-toolchain.toml /app/
COPY clippy.toml rustfmt.toml /app/
COPY src /app/src
COPY migrations /app/migrations

WORKDIR /app
RUN cargo build --release --locked --bin picrete-rust --bin worker

FROM debian:bullseye-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libssl1.1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/picrete-rust /usr/local/bin/picrete-rust
COPY --from=builder /app/target/release/worker /usr/local/bin/picrete-worker

EXPOSE 8000

CMD ["picrete-rust"]
