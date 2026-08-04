# object-storage-perf

A Rust and [Apache OpenDAL](https://opendal.apache.org/) benchmark suite for
S3-compatible object storage. It runs 2 MiB range reads, 512 MiB multipart
writes, and object stat requests, then prints a rough latency and throughput
report.

## Configuration

Set the connection through environment variables:

```console
export OSP_ENDPOINT=https://s3.us-east-1.amazonaws.com
export OSP_BUCKET=my-benchmark-bucket
export OSP_REGION=us-east-1
export OSP_ACCESS_KEY_ID=...
export OSP_SECRET_ACCESS_KEY=...
export OSP_PREFIX=object-storage-perf
```

Do not commit credentials. Environment variables are preferable to credential
flags because command-line values can be visible in process listings.

## Run

Run all three workloads:

```console
cargo run --release
```

Each workload runs for 10 seconds by default. Read and multipart part-write
limits default to 128 in-flight requests; stat defaults to four. Tune them to
find the backend limit:

```console
cargo run --release -- \
  --read-concurrency 128 \
  --write-concurrency 128 \
  --stat-concurrency 24 \
  --read-duration-seconds 30 \
  --write-duration-seconds 30 \
  --stat-duration-seconds 30
```

`--read-concurrency` limits range reads currently in progress.
`--write-concurrency` is one global limit across multipart part writes; it is
not multiplied by the number of object uploads. `--stat-concurrency` limits
stat requests.

OpenDAL retries temporary failures up to three times with jittered backoff.
Control-operation and per-I/O-attempt timeouts default to 30 seconds. Configure
them with `OSP_RETRY_MAX_TIMES`, `OSP_TIMEOUT_SECONDS`, and
`OSP_IO_TIMEOUT_SECONDS`.

Every write operation uses a new object path. The suite also creates one
512 MiB source object for read and stat. It attempts to delete all created
objects after reporting; use `--keep-objects` to retain them. Credentials
without delete permission can still run the benchmark, but cleanup will emit a
warning.

The measured phases use:

- Object size: 512 MiB
- Read size: 2 MiB, aligned within the source object
- Multipart upload part size: 10 MiB, with a final 2 MiB part
- Read throughput: successful bytes divided by measured wall time
- Write throughput: successfully completed object bytes divided by measured
  wall time
- Stat throughput: successful operations divided by measured wall time
- Latency: HDR histogram mean, p50, p95, and p99 for successful operations
- Reliability: success rate and final error counts grouped by OpenDAL error kind

The source-object preparation and final cleanup are outside measured phases.
At the duration deadline, in-flight reads and stat requests are canceled and
in-progress multipart writes are aborted. Multipart abort and final cleanup can
add a small amount of wall time after measurement stops. Use
`cargo run -- --help` for all flags and environment variables.

## Run on OKE

OKE workers are Linux/AMD64. On an Apple Silicon workstation, cross-compile a
static AMD64 binary locally with Zig, then let Docker package only that binary:

```console
brew install zig rustup docker-buildx
rustup toolchain install 1.91.1
rustup target add x86_64-unknown-linux-musl --toolchain 1.91.1
cargo install cargo-zigbuild

docker login phx.ocir.io
IMAGE_TAG=benchmark-001 bash scripts/build-image.sh
```

The default repository is
`phx.ocir.io/axnzj5nsewcd/object-storage-perf`. Override it with
`IMAGE_REPOSITORY`. Always use a unique immutable tag. To package and test
without pushing:

```console
PUSH_IMAGE=0 \
IMAGE_REPOSITORY=object-storage-perf \
IMAGE_TAG=local-amd64 \
bash scripts/build-image.sh

docker run --rm --platform linux/amd64 \
  object-storage-perf:local-amd64 --help
```

Create the benchmark credential Secret out of band. The access key and secret
must be an OCI Customer Secret Key when using Oracle's S3-compatible endpoint:

```console
kubectl -n default create secret generic object-storage-perf-s3 \
  --from-literal=access-key-id="$OSP_ACCESS_KEY_ID" \
  --from-literal=secret-access-key="$OSP_SECRET_ACCESS_KEY"
```

The Job expects an OCIR pull Secret named `ocir-secret`. Deploy the immutable
image and follow its report:

```console
BENCHMARK_IMAGE=phx.ocir.io/axnzj5nsewcd/object-storage-perf:benchmark-001 \
OSP_ENDPOINT=https://axnzj5nsewcd.compat.objectstorage.us-phoenix-1.oraclecloud.com \
OSP_BUCKET=my-benchmark-bucket \
OSP_REGION=us-phoenix-1 \
OSP_READ_DURATION=120 \
OSP_WRITE_DURATION=120 \
OSP_STAT_DURATION=30 \
OSP_READ_CONCURRENCY=128 \
OSP_WRITE_CONCURRENCY=128 \
OSP_STAT_CONCURRENCY=24 \
bash k8s/deploy-job.sh

kubectl -n default logs -f job/object-storage-perf
```

Use `NAMESPACE` or `IMAGE_PULL_SECRET` to override the Kubernetes defaults.
Neither the image build nor the Kubernetes Job compiles Rust.
