# syntax=docker/dockerfile:1
# Reshard relay — the `reshard-server` crate builds the `reshard-relay` binary.
# Built by Coolify (or CI) and run behind a reverse proxy at api.reshard.dev.

FROM rust:1-bookworm AS build
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --locked --release -p reshard-server \
    && cp target/release/reshard-relay /usr/local/bin/reshard-relay

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 reshard \
    && mkdir -p /data && chown reshard /data
COPY --from=build /usr/local/bin/reshard-relay /usr/local/bin/reshard-relay
USER reshard
# RESHARD_HOST=0.0.0.0 so the reverse proxy can reach it; DB on a persistent volume.
ENV RESHARD_HOST=0.0.0.0 \
    PORT=8787 \
    RESHARD_DB=/data/reshard.db
EXPOSE 8787
CMD ["reshard-relay"]
