FROM rust:slim-bullseye AS build

RUN apt-get update && apt-get install -y build-essential clang
WORKDIR /app
COPY . /app
RUN cargo build --locked --release --bin sonic-router
RUN strip ./target/release/sonic-router

FROM gcr.io/distroless/cc

WORKDIR /usr/src/sonic
COPY --from=build /app/target/release/sonic-router /usr/local/bin/sonic-router
CMD [ "sonic-router", "-c", "/etc/sonic-router.cfg" ]
EXPOSE 1490 1492
