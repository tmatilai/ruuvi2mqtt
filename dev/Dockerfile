FROM rust:1-bullseye

RUN mkdir /usr/src/app
WORKDIR /usr/src/app

RUN dpkg --add-architecture amd64 \
    && dpkg --add-architecture armhf \
    && apt-get update \
    && apt-get install -y \
        gcc-arm-linux-gnueabihf \
        gcc-x86-64-linux-gnu \
        libdbus-1-dev \
        libdbus-1-dev:amd64 \
        libdbus-1-dev:armhf \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add \
        armv7-unknown-linux-gnueabihf \
        x86_64-unknown-linux-gnu

COPY cargo-config.toml /.cargo/config.toml
