use clap::Parser;

use crate::config::{BenchmarkArgs, StorageArgs};

#[derive(Parser)]
#[command(
    name = "object-storage-perf",
    version,
    about = "Benchmark S3-compatible object storage with OpenDAL",
    after_help = "\
Examples:
  # Run with connection settings from the environment
  OSP_ENDPOINT=https://s3.example.com OSP_BUCKET=bench \\
    OSP_ACCESS_KEY_ID=... OSP_SECRET_ACCESS_KEY=... object-storage-perf

  # Tune workload duration and concurrency
  object-storage-perf --read-duration-seconds 30 --write-duration-seconds 30 \\
    --stat-duration-seconds 30 --read-concurrency 64 \\
    --write-concurrency 128 --stat-concurrency 24"
)]
pub struct Cli {
    #[command(flatten)]
    pub storage: StorageArgs,

    #[command(flatten)]
    pub benchmark: BenchmarkArgs,
}
