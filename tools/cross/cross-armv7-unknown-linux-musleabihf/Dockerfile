FROM rustembedded/cross:armv7-unknown-linux-musleabihf-0.2.0

RUN apt-get update && \
    apt-get install -y clang libclang-dev libc6-dev-i386 && \
    rm -rf /var/lib/apt/lists/*
