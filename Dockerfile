# syntax=docker/dockerfile:1
FROM rust:1.99.0-alpine3.24 AS build
RUN apk add --no-cache build-base cmake clang18 clang18-libclang nasm perl pkgconf linux-headers git \
    libstdc++-dev binutils && rustup component add rustfmt
# Host build scripts load libclang; only explicit target crates stay static.
RUN printf '%s\n' '#!/bin/sh' 'compiler="$1"; shift' \
    'for arg in "$@"; do' \
    '  case "$arg" in --target|--target=*) exec "$compiler" "$@";; esac' \
    'done' 'exec "$compiler" "$@" -C target-feature=-crt-static' \
    > /usr/local/bin/cf-rustc && chmod +x /usr/local/bin/cf-rustc
WORKDIR /app
ARG TARGETARCH
ARG BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=${BUILD_JOBS} CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 LIBCLANG_PATH=/usr/lib/llvm18/lib \
    RUSTC_WRAPPER=/usr/local/bin/cf-rustc LANG=en_US.UTF-8 TZ=Asia/Shanghai
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
COPY profiles ./profiles
COPY tests ./tests
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,id=cf-gateway-static-v2-${TARGETARCH},target=/app/target,sharing=locked \
    set -eu; \
    target="$(rustc -vV | sed -n 's/^host: //p')"; \
    case "$target" in x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;; *) exit 1 ;; esac; \
    cargo fmt --check; \
    cargo test --release --locked --target "$target"; \
    cargo build --release --locked --target "$target" --bin cf-gateway; \
    mkdir /out; \
    cp "target/$target/release/cf-gateway" /out/cf-gateway; \
    if readelf -l /out/cf-gateway | grep -q INTERP; then echo 'Binary requires a dynamic loader'; exit 1; fi; \
    if readelf -d /out/cf-gateway | grep -q NEEDED; then echo 'Binary requires shared libraries'; exit 1; fi

FROM gcr.io/distroless/static:nonroot AS runtime
COPY --from=build /out/cf-gateway /cf-gateway
COPY LICENSE LICENSES /licenses/
ENV LANG=en_US.UTF-8 TZ=Asia/Shanghai CF_GATEWAY_TIMEZONE=Asia/Shanghai
EXPOSE 19686
USER nonroot:nonroot
ENTRYPOINT ["/cf-gateway", "serve", "--host", "0.0.0.0", "--port", "19686"]
