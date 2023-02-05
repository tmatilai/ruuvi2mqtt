FROM scratch

ARG TARGETPLATFORM

COPY binaries/$TARGETPLATFORM/ruuvi2mqtt /

ENTRYPOINT ["/ruuvi2mqtt"]
