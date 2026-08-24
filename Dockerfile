FROM rust:1.88-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /src/target/release/spacl /usr/local/bin/spacl
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/spacl"]
CMD ["--help"]

