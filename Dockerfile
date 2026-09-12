FROM rust:1.88.0-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
COPY migrations ./migrations
ARG BUILD_REVISION=unknown
RUN cargo build --release --locked --bins

FROM ubuntu:24.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/*

ARG BUILD_REVISION=unknown
LABEL org.opencontainers.image.revision=$BUILD_REVISION
LABEL org.opencontainers.image.source="https://github.com/dorogao5/Picrete"

WORKDIR /app

COPY --from=builder /build/target/release/picrete-rust /usr/local/bin/picrete-rust
COPY --from=builder /build/target/release/worker /usr/local/bin/picrete-worker
COPY --from=builder /build/target/release/telegram_bot /usr/local/bin/picrete-telegram-bot
COPY migrations /app/migrations

EXPOSE 8000

CMD ["picrete-rust"]
