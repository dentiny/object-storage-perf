use anyhow::Result;
use clap::Parser;
use object_storage_perf::{
    cli::{Cli, Command},
    config::StorageConfig,
    storage::{CheckResult, Storage},
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = StorageConfig::try_from(cli.storage)?;
    let storage = Storage::new(config)?;

    match cli.command {
        Command::Check { object } => match storage.check(object.as_deref()).await? {
            CheckResult::Object {
                path,
                content_length,
            } => {
                println!("connection ok: object {path:?} is accessible ({content_length} bytes)");
            }
            CheckResult::Prefix {
                prefix,
                first_entry,
            } => match first_entry {
                Some(path) => {
                    println!("connection ok: listed prefix {prefix:?}; first entry is {path:?}");
                }
                None => {
                    println!("connection ok: listed prefix {prefix:?}; prefix is empty");
                }
            },
        },
    }

    Ok(())
}
