# Pinned toolchain and lockfile; build failures are never hidden.
FROM rust:1.98.1-bookworm AS builder
WORKDIR /usr/src/athena
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY src ./src
COPY benches ./benches
RUN cargo build --locked --release --bin athena-broker

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=builder /usr/src/athena/target/release/athena-broker /app/athena-broker
ENV PORT=8080 RUST_LOG=info
EXPOSE 8080
USER nonroot:nonroot
ENTRYPOINT ["/app/athena-broker"]
