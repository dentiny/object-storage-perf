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

Each workload runs for 10 seconds with four concurrent logical operations by
default. Tune concurrency and duration to find the backend limit:

```console
cargo run --release -- \
  --concurrency 16 \
  --duration-seconds 30 \
  --multipart-concurrency 2
```

`--concurrency` controls simultaneous read, write, or stat operations.
`--multipart-concurrency` controls simultaneous part requests inside each
512 MiB upload, so the write phase can issue up to the product of those two
settings in parallel.

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

The source-object preparation and final cleanup are outside measured phases.
At the duration deadline, in-flight reads and stat requests are canceled and
in-progress multipart writes are aborted. Multipart abort and final cleanup can
add a small amount of wall time after measurement stops. Use
`cargo run -- --help` for all flags and environment variables.
