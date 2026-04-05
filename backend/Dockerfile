# Stage 1: Build
FROM rust:1.94 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libpq5 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/oneclick-backend /usr/local/bin/
COPY --from=builder /app/migrations /app/migrations

EXPOSE 8080
CMD ["oneclick-backend"]
