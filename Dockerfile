# Stage 1: Build
FROM rust:1.86-bookworm AS builder

RUN apt-get update && apt-get install -y librrd-dev pkg-config libclang-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependencies by building with a stub main first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

# Build real source
COPY src/ src/
COPY templates/ templates/
COPY static/ static/
RUN touch src/main.rs && cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y ca-certificates librrd8 \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/airgradient /usr/local/bin/airgradient

VOLUME /data
ENV AIRGRADIENT_CONFIG=/data/config.toml

EXPOSE 8080

ENTRYPOINT ["airgradient"]
