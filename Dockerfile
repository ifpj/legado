# syntax=docker/dockerfile:1
FROM rust:1.97.1-bookworm AS build
RUN rustup component add rustfmt
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY source ./source
COPY web ./web
COPY tests/signature-vectors.json ./tests/signature-vectors.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo fmt --check && cargo test --locked && cargo build --release --locked

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/fanqie-relay /usr/local/bin/fanqie-relay
COPY LICENSE /usr/share/doc/fanqie-relay/LICENSE
ENV FANQIE_RELAY_LISTEN=0.0.0.0:19670
USER 10001:10001
EXPOSE 19670
STOPSIGNAL SIGINT
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl --fail --silent --show-error http://127.0.0.1:19670/health || exit 1
ENTRYPOINT ["/usr/local/bin/fanqie-relay"]
