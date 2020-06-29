FROM rustlang/rust:nightly-buster-slim AS build

RUN apt-get update
RUN apt-get install -y build-essential clang

WORKDIR /app
COPY . /app
RUN cargo clean && cargo build --release --target x86_64-unknown-linux-gnu
RUN strip ./target/x86_64-unknown-linux-gnu/release/sonic

FROM debian:buster-slim

WORKDIR /usr/src/sonic

COPY --from=build /app/target/x86_64-unknown-linux-gnu/release/sonic /usr/local/bin/sonic

CMD [ "sonic", "-c", "/etc/sonic.cfg" ]

EXPOSE 1491
