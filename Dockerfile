FROM rustlang/rust:nightly-slim AS build

WORKDIR /usr/src/sonic

RUN apt-get update
RUN apt-get install -y build-essential clang
RUN cargo install sonic-server

FROM debian:stretch-slim

COPY --from=build /usr/local/cargo/bin/sonic /usr/local/bin/sonic

CMD [ "sonic", "-c", "/etc/sonic.cfg" ]

EXPOSE 1491
