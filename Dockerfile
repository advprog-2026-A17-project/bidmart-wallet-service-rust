FROM rust:1.88-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y protobuf-compiler

COPY Cargo.toml Cargo.lock ./
COPY src src
COPY migrations migrations
COPY proto proto
COPY build.rs build.rs

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked \
    && cp target/release/bidmart-wallet-service-rust ./bidmart-wallet-service-rust

# Reuse the builder base (glibc bookworm) so Compose does not pull debian:bookworm-slim separately.
FROM rust:1.88-bookworm AS runtime

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/bidmart-wallet-service-rust /usr/local/bin/bidmart-wallet-service-rust

EXPOSE 8083

CMD ["bidmart-wallet-service-rust"]
