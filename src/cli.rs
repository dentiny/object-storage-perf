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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::config::{BenchmarkConfig, StorageConfig};

    #[test]
    fn parses_storage_and_benchmark_settings() {
        let cli = Cli::try_parse_from([
            "object-storage-perf",
            "--endpoint",
            "https://storage.example.com",
            "--bucket",
            "bench",
            "--access-key-id",
            "access",
            "--secret-access-key",
            "secret",
            "--concurrency",
            "16",
        ])
        .unwrap();

        assert_eq!(cli.benchmark.concurrency, 16);
    }

    #[test]
    fn requires_storage_credentials() {
        let cli = Cli::try_parse_from(["object-storage-perf"]).unwrap();
        let error = StorageConfig::try_from(cli.storage).err().unwrap();

        let message = error.to_string();
        assert!(message.contains("endpoint is required"));
    }

    #[test]
    fn validates_benchmark_settings() {
        let cli = Cli::try_parse_from(["object-storage-perf", "--concurrency", "0"]).unwrap();
        let error = BenchmarkConfig::try_from(cli.benchmark).err().unwrap();

        assert!(error.to_string().contains("concurrency"));
    }
}
