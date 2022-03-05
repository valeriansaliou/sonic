FROM rust:1.58.1-slim-bullseye AS build

RUN apt-get update
RUN apt-get install -y build-essential clang

RUN rustup --version
RUN rustup component add rustfmt

RUN rustc --version && \
    rustup --version && \
    cargo --version

WORKDIR /app
COPY . /app
RUN cargo clean && cargo build --release
RUN strip /app/target/release/sonic

# Copy Sonic executable to empty container to export it using Docker's --output option
FROM scratch as exporter
COPY --from=build /app/target/release/sonic /
