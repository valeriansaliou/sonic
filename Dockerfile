FROM rustlang/rust:nightly-slim

WORKDIR /usr/src/sonic

RUN apt-get update
RUN apt-get install -y build-essential clang
RUN cargo install sonic-server
CMD [ "sonic", "-c", "/etc/sonic.cfg" ]

EXPOSE 1491
