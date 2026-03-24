FROM rust:1.88-alpine AS builder
RUN apk add --no-cache musl-dev

WORKDIR /app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && touch src/lib.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# Build real binary
COPY src/ src/
RUN touch src/main.rs src/lib.rs && cargo build --release

FROM alpine:3.21
COPY --from=builder /app/target/release/boxer /usr/local/bin/boxer
ENTRYPOINT ["/usr/local/bin/boxer"]
