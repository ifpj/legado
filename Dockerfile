# syntax=docker/dockerfile:1
FROM rust:1.97.1-alpine AS build
RUN apk add --no-cache binutils && rustup component add rustfmt
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY source ./source
COPY web ./web
COPY tests/signature-vectors.json ./tests/signature-vectors.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo fmt --check && cargo test --locked && cargo build --release --locked \
    && if readelf -l target/release/fanqie-relay | grep -q INTERP; then echo 'Binary is not static'; exit 1; fi \
    && if readelf -d target/release/fanqie-relay | grep -q NEEDED; then echo 'Binary requires shared libraries'; exit 1; fi

FROM gcr.io/distroless/static:nonroot AS runtime
COPY --from=build /build/target/release/fanqie-relay /usr/local/bin/fanqie-relay
COPY LICENSE /usr/share/doc/fanqie-relay/LICENSE
ENV FANQIE_RELAY_LISTEN=0.0.0.0:19670
EXPOSE 19670
STOPSIGNAL SIGINT
ENTRYPOINT ["/usr/local/bin/fanqie-relay"]
