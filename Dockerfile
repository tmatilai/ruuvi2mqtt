FROM debian:11-slim

ARG TARGETPLATFORM

RUN apt update \
    && apt install -y dbus \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY binaries/$TARGETPLATFORM/ruuvi2mqtt /app/

ENTRYPOINT ["/app/ruuvi2mqtt"]
