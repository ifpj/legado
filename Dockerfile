# syntax=docker/dockerfile:1
FROM rust:1.99.0-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake clang libclang-dev nasm pkg-config libicu-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
ENV CARGO_BUILD_JOBS=2 LANG=en_US.UTF-8 TZ=Asia/Shanghai
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
COPY profiles ./profiles
COPY tests ./tests
RUN cargo fmt --check && cargo test --locked
RUN cargo build --locked --release --bin cf-gateway \
    && mkdir /out \
    && cp target/release/cf-gateway /out/cf-gateway \
    && cp -L /usr/lib/$(dpkg-architecture -qDEB_HOST_MULTIARCH)/libicui18n.so.72 /out/ \
    && cp -L /usr/lib/$(dpkg-architecture -qDEB_HOST_MULTIARCH)/libicuuc.so.72 /out/ \
    && cp -L /usr/lib/$(dpkg-architecture -qDEB_HOST_MULTIARCH)/libicudata.so.72 /out/ \
    && cp /usr/share/doc/libicu72/copyright /out/icu-copyright

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /out/cf-gateway /cf-gateway
COPY --from=build /out/libicu* /usr/lib/
COPY --from=build /out/icu-copyright /licenses/icu-copyright
COPY LICENSE LICENSES /licenses/
ENV LANG=en_US.UTF-8 TZ=Asia/Shanghai CF_GATEWAY_TIMEZONE=Asia/Shanghai
EXPOSE 19686
USER nonroot:nonroot
ENTRYPOINT ["/cf-gateway", "serve", "--host", "0.0.0.0", "--port", "19686"]
