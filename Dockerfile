FROM scratch

ARG TARGETPLATFORM

# CA store for `tls: true` without `ca_file`
COPY --from=alpine:3.22 /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

COPY binaries/$TARGETPLATFORM/ruuvi2mqtt /

ENTRYPOINT ["/ruuvi2mqtt"]
