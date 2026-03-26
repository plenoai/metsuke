FROM rust:1.86 AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release --bin metsuke

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/metsuke /usr/local/bin/
ENV PORT=8080
EXPOSE 8080
CMD ["metsuke"]
