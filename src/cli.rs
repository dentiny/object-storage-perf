use clap::{Parser, Subcommand};

use crate::config::StorageArgs;

#[derive(Parser)]
#[command(
    name = "object-storage-perf",
    version,
    about = "Benchmark S3-compatible object storage with OpenDAL"
)]
pub struct Cli {
    #[command(flatten)]
    pub storage: StorageArgs,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Validate capabilities and perform a read-only connectivity check.
    Check {
        /// Stat this object instead of listing one entry under the benchmark prefix.
        #[arg(long)]
        object: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::config::{AddressingStyle, StorageConfig};

    #[test]
    fn parses_storage_settings_and_check_command() {
        let cli = Cli::try_parse_from([
            "object-storage-perf",
            "check",
            "--endpoint",
            "https://storage.example.com",
            "--bucket",
            "bench",
            "--access-key-id",
            "access",
            "--secret-access-key",
            "secret",
            "--addressing-style",
            "virtual-hosted",
            "--object",
            "existing/object",
        ])
        .unwrap();

        assert_eq!(cli.storage.addressing_style, AddressingStyle::VirtualHosted);
        assert!(matches!(
            cli.command,
            Command::Check {
                object: Some(ref path)
            } if path == "existing/object"
        ));
    }

    #[test]
    fn requires_storage_credentials() {
        let cli = Cli::try_parse_from(["object-storage-perf", "check"]).unwrap();
        let error = StorageConfig::try_from(cli.storage).err().unwrap();

        let message = error.to_string();
        assert!(message.contains("endpoint is required"));
    }
}
