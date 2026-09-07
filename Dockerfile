FROM ubuntu:24.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/*

ARG BUILD_REVISION=unknown
LABEL org.opencontainers.image.revision=$BUILD_REVISION
LABEL org.opencontainers.image.source="https://github.com/dorogao5/Picrete"

WORKDIR /app

COPY target/release/picrete-rust /usr/local/bin/picrete-rust
COPY target/release/worker /usr/local/bin/picrete-worker
COPY target/release/telegram_bot /usr/local/bin/picrete-telegram-bot
COPY migrations /app/migrations

EXPOSE 8000

CMD ["picrete-rust"]
