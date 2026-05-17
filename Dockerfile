FROM rust:1-slim-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --home-dir /nonexistent --shell /usr/sbin/nologin pcloud

COPY --from=builder /app/target/release/pcloud-relay-server /usr/local/bin/pcloud-relay-server

ENV PCLOUD_RELAY_BIND=0.0.0.0:7070

EXPOSE 7070

USER pcloud

ENTRYPOINT ["pcloud-relay-server"]
