FROM rust:slim-bullseye AS build

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
RUN strip ./target/release/sonic

FROM gcr.io/distroless/cc

WORKDIR /usr/src/sonic

COPY --from=build /app/target/release/sonic /usr/local/bin/sonic

RUN ln -s /run/sonic/sonic.cfg /etc/sonic.cfg
VOLUME /run/sonic

CMD [ "sonic", "-c", "/etc/sonic.cfg" ]

EXPOSE 1491
