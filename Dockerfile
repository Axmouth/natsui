FROM rust:1.94-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web
COPY deploy ./deploy
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/app/target cargo build --release --locked && cp target/release/natsui /app/natsui

FROM build AS verification
COPY --from=node:22-bookworm-slim /usr/local/bin/node /usr/local/bin/node
COPY --from=nats:2.11.8-alpine /usr/local/bin/nats-server /usr/local/bin/nats-server
COPY scripts ./scripts
COPY *.md LICENSE ./
COPY docs ./docs
COPY demo/README.md ./demo/README.md
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/app/target \
    NATSUI_TLS_FIXTURES="$(node scripts/tls-fixtures.mjs)" NATSUI_TEST_SERVER=/usr/local/bin/nats-server cargo test --locked -- --include-ignored \
    && node scripts/test-trends.mjs && node scripts/test-demo.mjs && node scripts/package.mjs

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home natsui \
    && install -d -o 10001 -g 10001 /data
COPY --from=build /app/natsui /usr/local/bin/natsui
COPY LICENSE /usr/share/doc/natsui/LICENSE
USER 10001:10001
ENV NATSUI_CONTAINER=1 NATSUI_DATA_DIR=/data
EXPOSE 4321
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:4321/healthz || exit 1
ENTRYPOINT ["natsui"]
