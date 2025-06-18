FROM rust:1.85 AS builder

RUN apt-get update && apt-get install -y libpq-dev

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

COPY . .

COPY public /app/public
COPY .sqlx .sqlx

ENV SQLX_OFFLINE=true

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libpq5 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/notifyhub ./notifyhub
COPY --from=builder /app/public ./public

EXPOSE 8080

CMD ["./notifyhub"]
