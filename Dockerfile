FROM gcr.io/distroless/static-debian12:nonroot

# scripts/build-image.sh cross-compiles this static Linux/AMD64 binary locally.
# Docker only packages it; no Rust compilation happens in the image build.
COPY object-storage-perf /usr/local/bin/object-storage-perf

USER nonroot
ENTRYPOINT ["/usr/local/bin/object-storage-perf"]
