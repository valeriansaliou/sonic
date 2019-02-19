FROM rustlang/rust:nightly-slim

WORKDIR /usr/src/sonic

RUN cargo install sonic-server
CMD [ "sonic", "-c", "/etc/sonic.cfg" ]

EXPOSE 1491
