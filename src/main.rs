use anyhow::Result;
use clap::Parser;
use object_storage_perf::{
    benchmark::{BenchmarkSuite, MULTIPART_PART_SIZE, OBJECT_SIZE, READ_SIZE},
    cli::Cli,
    config::{BenchmarkConfig, StorageConfig},
    storage::Storage,
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = StorageConfig::try_from(cli.storage)?;
    let benchmark_config = BenchmarkConfig::try_from(cli.benchmark)?;
    let duration_seconds = benchmark_config.duration_seconds;
    let concurrency = benchmark_config.concurrency;
    let multipart_concurrency = benchmark_config.multipart_concurrency;
    let storage = Storage::new(config)?;
    let benchmark = BenchmarkSuite::new(storage, benchmark_config);

    println!("object storage benchmark");
    println!(
        "object: {} MiB | read: {} MiB | multipart part: {} MiB",
        OBJECT_SIZE / (1024 * 1024),
        READ_SIZE / (1024 * 1024),
        MULTIPART_PART_SIZE / (1024 * 1024)
    );
    println!(
        "duration: {duration_seconds}s per workload | concurrency: {concurrency} | multipart concurrency: {multipart_concurrency}"
    );

    let reports = benchmark.run().await?;

    println!();
    println!("performance report");
    for report in reports {
        println!("{report}");
    }

    Ok(())
}
