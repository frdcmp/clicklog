# ── build ────────────────────────────────────────────────────────────────────
FROM rust:1-slim AS build
WORKDIR /app
# Cache deps: copy manifest, build a stub, then the real sources.
COPY Cargo.toml ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
# Touch so cargo rebuilds the real binary (stub main.rs had an older mtime).
RUN touch src/main.rs && cargo build --release

# ── runtime ──────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/ingest-api /usr/local/bin/ingest-api
EXPOSE 8080
CMD ["ingest-api"]
