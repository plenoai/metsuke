FROM rust:1.88-slim@sha256:a6cab604fa016ac022e78c24038497eb7617ab59150ca4c3dd2ede0fbd514d4b AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev git build-essential && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN git init && git config user.email "build@build" && git config user.name "build" && git commit --allow-empty -m "build"
RUN cargo build --release --bin metsuke

FROM debian:bookworm-slim@sha256:8af0e5095f9964007f5ebd11191dfe52dcb51bf3afa2c07f055fc5451b78ba0e
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/metsuke /usr/local/bin/
ENV PORT=8080
EXPOSE 8080
CMD ["metsuke"]
