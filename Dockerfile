FROM rust:1.88-slim AS chef
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev git build-essential && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
COPY --from=planner /app/Cargo.lock Cargo.lock
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN git init && git config user.email "build@build" && git config user.name "build" && git commit --allow-empty -m "build"
RUN cargo build --release --bin metsuke

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -m -u 1000 metsuke
COPY --from=builder /app/target/release/metsuke /usr/local/bin/
COPY --from=builder /app/crates/server/static /app/static
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh && chown -R metsuke:metsuke /app
ENV PORT=8080
ENV STATIC_DIR=/app/static
EXPOSE 8080
CMD ["entrypoint.sh"]
