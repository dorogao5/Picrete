FROM ubuntu:24.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY target/release/picrete-rust /usr/local/bin/picrete-rust
COPY target/release/worker /usr/local/bin/picrete-worker

EXPOSE 8000

CMD ["picrete-rust"]
