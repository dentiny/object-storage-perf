# object-storage-perf

A Rust and [Apache OpenDAL](https://opendal.apache.org/) benchmark suite for
S3-compatible object storage. The benchmark workloads will be added in the next
milestone; the current CLI configures storage and performs a read-only
connectivity check.

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

## Connectivity check

Build and list at most one entry from the benchmark prefix:

```console
cargo run --release -- check
```

If the credentials cannot list the bucket, check a known object instead:

```console
cargo run --release -- check --object path/to/existing-object
```

Run `cargo run -- --help` to see the equivalent command-line configuration
flags.
