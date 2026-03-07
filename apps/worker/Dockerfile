FROM rust:1.92 AS build
WORKDIR /app

COPY Cargo.toml Cargo.toml
COPY apps/api/Cargo.toml apps/api/Cargo.toml
COPY apps/worker/Cargo.toml apps/worker/Cargo.toml
COPY crates/engine/Cargo.toml crates/engine/Cargo.toml
RUN mkdir -p apps/api/src apps/worker/src crates/engine/src && \
    printf "fn main() {}\n" > apps/api/src/main.rs && \
    printf "fn main() {}\n" > apps/worker/src/main.rs && \
    printf "pub fn placeholder() {}\n" > crates/engine/src/lib.rs && \
    cargo build --release -p workflow-worker

COPY . .
RUN cargo build --release -p workflow-worker

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/workflow-worker /usr/local/bin/workflow-worker
CMD ["workflow-worker"]
