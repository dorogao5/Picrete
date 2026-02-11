FROM ubuntu:24.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY target/release/picrete-rust /usr/local/bin/picrete-rust
COPY target/release/worker /usr/local/bin/picrete-worker
COPY target/release/telegram_bot /usr/local/bin/picrete-telegram-bot
COPY migrations /app/migrations

EXPOSE 8000

CMD ["picrete-rust"]
