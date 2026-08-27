# object-storage-perf

A Rust and [Apache OpenDAL](https://opendal.apache.org/) benchmark suite for
S3-compatible object storage. It runs 10 MiB range reads, 512 MiB multipart
writes, and object stat requests, then prints a rough latency and throughput
report.

See [Performance investigation](PERFORMANCE.md) for the bottlenecks found during
profiling, the fixes, and the final Crusoe scaling results.

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

Each workload starts requests for 10 seconds by default, then lets requests
already in flight finish. Read concurrency defaults to 64, multipart part-write
concurrency defaults to 128, and stat defaults to four. Tune them to find the
backend limit:

```console
cargo run --release -- \
  --read-concurrency 64 \
  --write-concurrency 128 \
  --stat-concurrency 24 \
  --read-duration-seconds 30 \
  --write-duration-seconds 30 \
  --stat-duration-seconds 30
```

`--read-concurrency` limits range reads currently in progress. The suite
prepares one source object per read worker and distributes reads across all of
them to avoid measuring a single hot object's limit.
`--write-concurrency` is one global limit across multipart part writes; it is
not multiplied by the number of object uploads. `--stat-concurrency` limits
stat requests.

OpenDAL retries temporary failures up to three times with jittered backoff.
Control-operation and per-I/O-attempt timeouts default to 30 seconds. Configure
them with `OSP_RETRY_MAX_TIMES`, `OSP_TIMEOUT_SECONDS`, and
`OSP_IO_TIMEOUT_SECONDS`.

Every write operation uses a new object path. Outside the measured phase, the
suite also creates one 512 MiB source object per read worker and spreads reads
across all of them. Stat uses one of those objects. It attempts to delete all
created objects after reporting; use `--keep-objects` to retain them.
Credentials without delete permission can still run the benchmark, but cleanup
will emit a warning.

The measured phases use:

- Object size: 512 MiB
- Read size: 10 MiB, aligned within the source object
- Multipart upload part size: 10 MiB, with a final 2 MiB part
- Read throughput: successful bytes divided by measured wall time
- Write throughput: successfully completed multipart part bytes divided by
  measured wall time
- Stat throughput: successful operations divided by measured wall time
- Latency: HDR histogram mean, p50, p95, and p99 for successful reads,
  multipart part writes, and stat operations
- Reliability: success rates for measured read, part-write, and stat operations;
  multipart start, completion, abort, and coordination failures are reported
  separately as control errors

The source-object preparation and final cleanup are outside measured phases.
The configured duration is the request-start window: after its deadline, no
new measured operations begin, while operations already in flight finish or
reach their configured timeout. Measured wall time extends through the last
completed in-flight operation. Multipart completion, abort, and final cleanup
are excluded from that time. Use
`cargo run -- --help` for all flags and environment variables.

## Build the container image

Crusoe and OKE workers are Linux/AMD64. On an Apple Silicon workstation,
cross-compile a static AMD64 binary locally with Zig, then let Docker package
only that binary.
The benchmark uses jemalloc to avoid allocator-induced `mmap` contention while
processing many network buffers concurrently:

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

Create the benchmark credential Secret out of band in each target cluster:

```console
kubectl -n default create secret generic object-storage-perf-s3 \
  --from-literal=access-key-id="$OSP_ACCESS_KEY_ID" \
  --from-literal=secret-access-key="$OSP_SECRET_ACCESS_KEY"
```

The Job expects an image pull Secret named `ocir-secret` by default. Override
it with `IMAGE_PULL_SECRET`.

## Run on Crusoe

The Crusoe deployment defaults to context `hark-norway-gpu`, the private
`hjiang-test-bucket`, and the Object Storage endpoint in `eu-norway1-a`. The
endpoint is reachable only from Crusoe compute in that location. Pinning the
Job to one worker keeps performance comparisons reproducible:

```console
BENCHMARK_IMAGE=phx.ocir.io/axnzj5nsewcd/object-storage-perf:benchmark-001 \
NODE_NAME=np-b69a6ebc-3.eu-norway1-a.compute.internal \
CPU_LIMIT=64 \
TOKIO_WORKER_THREADS=64 \
bash k8s/deploy-crusoe-job.sh

kubectl --context hark-norway-gpu -n default logs -f job/object-storage-perf
```

Override `KUBE_CONTEXT`, `OSP_ENDPOINT`, `OSP_BUCKET`, or `OSP_REGION` when
targeting a different Crusoe cluster or location. Use a dedicated Crusoe Object
Storage API key in `object-storage-perf-s3`.

## Run on OCI

The OCI deployment defaults to context `oke-phx-gpu`, region `us-phoenix-1`,
and node type `BM.GPU.B4.8`. Supply the S3-compatible endpoint and bucket:

```console
BENCHMARK_IMAGE=phx.ocir.io/axnzj5nsewcd/object-storage-perf:benchmark-001 \
OSP_ENDPOINT=https://axnzj5nsewcd.compat.objectstorage.us-phoenix-1.oraclecloud.com \
OSP_BUCKET=my-benchmark-bucket \
OSP_READ_DURATION=120 \
OSP_WRITE_DURATION=120 \
OSP_STAT_DURATION=30 \
OSP_READ_CONCURRENCY=64 \
OSP_WRITE_CONCURRENCY=128 \
OSP_STAT_CONCURRENCY=24 \
bash k8s/deploy-oci-job.sh

kubectl --context oke-phx-gpu -n default logs -f job/object-storage-perf
```

OCI requires a Customer Secret Key in `object-storage-perf-s3`. The selected
node must have the `object-storage-perf/dedicated=true` label and matching
`NoSchedule` taint. Override `NODE_TYPE` when using another OKE worker shape.

Both deployment scripts accept the same workload, resource, namespace, and
image-pull overrides. Neither script compiles Rust.
