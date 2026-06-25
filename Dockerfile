FROM rust:1.85-slim-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY tunnel-proto/ tunnel-proto/
COPY tunnel-server/ tunnel-server/
RUN apt-get update && apt-get install -y pkg-config libssl-dev && \
    cargo build --release -p tunnel-server && \
    cp target/release/tunnel-server /tunnel-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /tunnel-server /usr/local/bin/tunnel-server
EXPOSE 443 9000
ENTRYPOINT ["tunnel-server"]
