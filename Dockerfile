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

FROM debian:bullseye-slim

COPY --from=build /app/target/release/sonic /usr/local/bin/sonic

EXPOSE 1491
CMD [ "sonic", "-c", "/etc/sonic.cfg" ]
