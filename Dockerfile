# Use Docker buildx's --target parameter to determine what to build:
#
# To build a Sonic executable for each specified platform and export it to the build host:
# docker buildx build . --target exporter --output type=local,dest=./bin --platforms linux/amd64
#
# To build a Docker image for each specified platform and push it to a registry:
# docker buildx build . --platforms linux/amd64,linux/arm64/v8 -t myorg/myrepo:latest

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
FROM scratch as export
COPY --from=build /app/target/release/sonic /

# Or build a full image
FROM debian:bullseye-slim as image

COPY --from=build /app/target/release/sonic /usr/local/bin/sonic
EXPOSE 1491
CMD [ "sonic", "-c", "/etc/sonic.cfg" ]
