use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use opendal::{Buffer, Operator};

use crate::{
    config::BenchmarkConfig, metrics::BenchmarkReport, storage::Storage, utils::zero_buffer,
};

mod read;
mod stats;
mod write;

pub const MIB: u64 = 1024 * 1024;
pub const OBJECT_SIZE: u64 = 512 * MIB;
pub const READ_SIZE: u64 = 10 * MIB;
pub const MULTIPART_PART_SIZE: u64 = 10 * MIB;

pub struct BenchmarkSuite {
    operator: Operator,
    prefix: String,
    config: BenchmarkConfig,
    full_part: Buffer,
    final_part: Buffer,
}

impl BenchmarkSuite {
    pub fn new(storage: Storage, config: BenchmarkConfig) -> Self {
        Self {
            operator: storage.operator().clone(),
            prefix: storage.prefix().to_owned(),
            config,
            full_part: zero_buffer(MULTIPART_PART_SIZE as usize),
            final_part: zero_buffer((OBJECT_SIZE % MULTIPART_PART_SIZE) as usize),
        }
    }

    pub async fn run(&self) -> Result<Vec<BenchmarkReport>> {
        let run_prefix = format!("{}/{}", self.prefix, run_id());
        let result = self.run_workloads(&run_prefix).await;

        if self.config.keep_objects {
            eprintln!("retaining benchmark objects under {run_prefix:?}");
        } else {
            eprintln!("cleaning up benchmark objects under {run_prefix:?}...");
            if let Err(error) = self.cleanup(&run_prefix).await {
                eprintln!("warning: benchmark cleanup failed: {error:#}");
            }
        }

        result
    }

    async fn run_workloads(&self, run_prefix: &str) -> Result<Vec<BenchmarkReport>> {
        let source_paths = (0..self.config.read_concurrency)
            .map(|index| format!("{run_prefix}/read-sources/object-{index}"))
            .collect::<Vec<_>>();

        eprintln!(
            "preparing {} source objects of 512 MiB each...",
            source_paths.len()
        );
        self.prepare_read_sources(&source_paths).await?;

        eprintln!(
            "running read workload for {}s at concurrency {} across {} objects...",
            self.config.read_duration_seconds,
            self.config.read_concurrency,
            source_paths.len()
        );
        let read = self.run_read(source_paths.clone()).await?;

        eprintln!(
            "running write workload for {}s at concurrency {}...",
            self.config.write_duration_seconds, self.config.write_concurrency
        );
        let write = self.run_write(run_prefix).await?;

        eprintln!(
            "running stat workload for {}s at concurrency {}...",
            self.config.stat_duration_seconds, self.config.stat_concurrency
        );
        let stat = self.run_stat(&source_paths[0]).await?;

        Ok(vec![read, write, stat])
    }

    async fn cleanup(&self, run_prefix: &str) -> Result<()> {
        let run_prefix = format!("{run_prefix}/");
        self.operator
            .delete_with(&run_prefix)
            .recursive(true)
            .await
            .with_context(|| format!("failed to delete run prefix {run_prefix:?}"))?;
        Ok(())
    }
}

fn run_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("run-{timestamp}-{}", std::process::id())
}
