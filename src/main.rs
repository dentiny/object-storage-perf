use anyhow::Result;
use clap::Parser;
use object_storage_perf::{
    benchmark::{BenchmarkSuite, MULTIPART_PART_SIZE, OBJECT_SIZE, READ_SIZE},
    cli::Cli,
    config::{BenchmarkConfig, StorageConfig},
    storage::Storage,
};

#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = StorageConfig::try_from(cli.storage)?;
    let benchmark_config = BenchmarkConfig::try_from(cli.benchmark)?;
    let read_duration = benchmark_config.read_duration_seconds;
    let write_duration = benchmark_config.write_duration_seconds;
    let stat_duration = benchmark_config.stat_duration_seconds;
    let read_concurrency = benchmark_config.read_concurrency;
    let write_concurrency = benchmark_config.write_concurrency;
    let stat_concurrency = benchmark_config.stat_concurrency;
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
        "duration read/write/stat: {read_duration}/{write_duration}/{stat_duration}s | in-flight read/part-write/stat: {read_concurrency}/{write_concurrency}/{stat_concurrency}"
    );

    let reports = benchmark.run().await?;

    println!();
    println!("performance report");
    for report in reports {
        println!("{report}");
    }

    Ok(())
}
