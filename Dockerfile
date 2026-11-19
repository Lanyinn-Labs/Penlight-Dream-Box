# Penlight Dream Box — multi-stage build
FROM rust:1-slim-trixie AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY web ./web
RUN cargo build --release --locked

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/penlight-dream-box /usr/local/bin/penlight-dream-box
EXPOSE 8080
ENV BOX_HOST=0.0.0.0
ENV BOX_PORT=8080
CMD ["penlight-dream-box"]
