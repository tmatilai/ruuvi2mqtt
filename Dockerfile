FROM scratch

ARG TARGETPLATFORM

# CA store for `tls: true` without `ca_file`
COPY --from=alpine:3.22 /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

COPY binaries/$TARGETPLATFORM/ruuvi2mqtt /

# BlueZ >= 5.51 lets any user call org.bluez on the system bus
USER 65534:65534

ENTRYPOINT ["/ruuvi2mqtt"]
