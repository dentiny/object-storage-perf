use clap::Parser;

use crate::config::{BenchmarkArgs, StorageArgs};

#[derive(Parser)]
#[command(
    name = "object-storage-perf",
    version,
    about = "Benchmark S3-compatible object storage with OpenDAL"
)]
pub struct Cli {
    #[command(flatten)]
    pub storage: StorageArgs,

    #[command(flatten)]
    pub benchmark: BenchmarkArgs,
}
