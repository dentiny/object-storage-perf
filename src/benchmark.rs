use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use futures_util::{StreamExt, stream};
use opendal::{Buffer, Operator};
use tokio::task::JoinSet;

use crate::{
    config::BenchmarkConfig,
    metrics::{BenchmarkReport, MetricsRecorder},
    storage::Storage,
};

pub const MIB: u64 = 1024 * 1024;
pub const OBJECT_SIZE: u64 = 512 * MIB;
pub const READ_SIZE: u64 = 2 * MIB;
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
            full_part: deterministic_buffer(MULTIPART_PART_SIZE as usize, 0x5eed),
            final_part: deterministic_buffer((OBJECT_SIZE % MULTIPART_PART_SIZE) as usize, 0x51de),
        }
    }

    pub async fn run(&self) -> Result<Vec<BenchmarkReport>> {
        let run_prefix = format!("{}/{}", self.prefix, run_id());
        let source_path = format!("{run_prefix}/read-source");
        let mut owned_paths = vec![source_path.clone()];

        eprintln!("preparing 512 MiB source object {source_path:?}...");
        self.upload_object(&source_path)
            .await
            .context("failed to prepare read/stat source object")?;

        eprintln!(
            "running read workload for {}s at concurrency {}...",
            self.config.duration_seconds, self.config.concurrency
        );
        let read = self.run_read(&source_path).await?;

        eprintln!(
            "running write workload for {}s at concurrency {}...",
            self.config.duration_seconds, self.config.concurrency
        );
        let (write, write_paths) = self.run_write(&run_prefix).await?;
        owned_paths.extend(write_paths);

        eprintln!(
            "running stat workload for {}s at concurrency {}...",
            self.config.duration_seconds, self.config.concurrency
        );
        let stat = self.run_stat(&source_path).await?;

        if self.config.keep_objects {
            eprintln!("retaining benchmark objects under {run_prefix:?}");
        } else {
            eprintln!("cleaning up {} benchmark objects...", owned_paths.len());
            if let Err(error) = self.cleanup(owned_paths).await {
                eprintln!("warning: benchmark completed but cleanup failed: {error:#}");
            }
        }

        Ok(vec![read, write, stat])
    }

    async fn run_read(&self, path: &str) -> Result<BenchmarkReport> {
        let recorder = Arc::new(MetricsRecorder::default());
        let path = Arc::new(path.to_owned());
        let started = Instant::now();
        let deadline = started + Duration::from_secs(self.config.duration_seconds);
        let mut workers = JoinSet::new();

        for worker_id in 0..self.config.concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let path = Arc::clone(&path);
            let concurrency = self.config.concurrency as u64;

            workers.spawn(async move {
                let mut block = worker_id as u64;
                while Instant::now() < deadline {
                    let offset = (block % (OBJECT_SIZE / READ_SIZE)) * READ_SIZE;
                    block += concurrency;
                    let operation_started = Instant::now();
                    let result = operator
                        .read_with(path.as_str())
                        .range(offset..offset + READ_SIZE)
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|buffer| {
                            if buffer.len() as u64 == READ_SIZE {
                                Ok(())
                            } else {
                                Err(anyhow!(
                                    "short read at offset {offset}: expected {READ_SIZE} bytes, got {}",
                                    buffer.len()
                                ))
                            }
                        });

                    match result {
                        Ok(()) => {
                            recorder.record_success(operation_started.elapsed(), READ_SIZE);
                        }
                        Err(error) => recorder.record_error(&error),
                    }
                }
            });
        }

        join_workers(&mut workers, "read").await?;
        Ok(recorder.report("read", started.elapsed()))
    }

    async fn run_write(&self, run_prefix: &str) -> Result<(BenchmarkReport, Vec<String>)> {
        let recorder = Arc::new(MetricsRecorder::default());
        let run_prefix = Arc::new(run_prefix.to_owned());
        let started = Instant::now();
        let deadline = started + Duration::from_secs(self.config.duration_seconds);
        let mut workers = JoinSet::new();

        for worker_id in 0..self.config.concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let run_prefix = Arc::clone(&run_prefix);
            let full_part = self.full_part.clone();
            let final_part = self.final_part.clone();
            let multipart_concurrency = self.config.multipart_concurrency;

            workers.spawn(async move {
                let mut sequence = 0_u64;
                let mut paths = Vec::new();

                while Instant::now() < deadline {
                    let path = format!(
                        "{}/writes/worker-{worker_id}/object-{sequence}",
                        run_prefix.as_str()
                    );
                    sequence += 1;
                    let operation_started = Instant::now();
                    let result = upload_object(
                        &operator,
                        &path,
                        multipart_concurrency,
                        &full_part,
                        &final_part,
                    )
                    .await;

                    match result {
                        Ok(()) => {
                            recorder.record_success(operation_started.elapsed(), OBJECT_SIZE);
                            paths.push(path);
                        }
                        Err(error) => recorder.record_error(&error),
                    }
                }

                paths
            });
        }

        let mut paths = Vec::new();
        while let Some(result) = workers.join_next().await {
            paths.extend(result.context("write benchmark worker failed")?);
        }

        Ok((recorder.report("write", started.elapsed()), paths))
    }

    async fn run_stat(&self, path: &str) -> Result<BenchmarkReport> {
        let recorder = Arc::new(MetricsRecorder::default());
        let path = Arc::new(path.to_owned());
        let started = Instant::now();
        let deadline = started + Duration::from_secs(self.config.duration_seconds);
        let mut workers = JoinSet::new();

        for _ in 0..self.config.concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let path = Arc::clone(&path);

            workers.spawn(async move {
                while Instant::now() < deadline {
                    let operation_started = Instant::now();
                    let result = operator
                        .stat(path.as_str())
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|metadata| {
                            if metadata.content_length() == OBJECT_SIZE {
                                Ok(())
                            } else {
                                Err(anyhow!(
                                    "unexpected object size: expected {OBJECT_SIZE}, got {}",
                                    metadata.content_length()
                                ))
                            }
                        });

                    match result {
                        Ok(()) => recorder.record_success(operation_started.elapsed(), 0),
                        Err(error) => recorder.record_error(&error),
                    }
                }
            });
        }

        join_workers(&mut workers, "stat").await?;
        Ok(recorder.report("stat", started.elapsed()))
    }

    async fn upload_object(&self, path: &str) -> Result<()> {
        upload_object(
            &self.operator,
            path,
            self.config.multipart_concurrency,
            &self.full_part,
            &self.final_part,
        )
        .await
    }

    async fn cleanup(&self, paths: Vec<String>) -> Result<()> {
        let operator = self.operator.clone();
        let concurrency = self.config.concurrency;
        let results = stream::iter(paths)
            .map(move |path| {
                let operator = operator.clone();
                async move {
                    operator
                        .delete(&path)
                        .await
                        .with_context(|| format!("failed to delete {path:?}"))
                }
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;

        let errors = results
            .into_iter()
            .filter_map(Result::err)
            .map(|error| format!("{error:#}"))
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow!(errors.join("; ")))
        }
    }
}

async fn upload_object(
    operator: &Operator,
    path: &str,
    multipart_concurrency: usize,
    full_part: &Buffer,
    final_part: &Buffer,
) -> Result<()> {
    let mut writer = operator
        .writer_with(path)
        .chunk(MULTIPART_PART_SIZE as usize)
        .concurrent(multipart_concurrency)
        .await
        .with_context(|| format!("failed to start multipart upload for {path:?}"))?;

    for _ in 0..(OBJECT_SIZE / MULTIPART_PART_SIZE) {
        if let Err(error) = writer.write(full_part.clone()).await {
            let _ = writer.abort().await;
            return Err(error).with_context(|| format!("failed to upload part for {path:?}"));
        }
    }

    if !final_part.is_empty()
        && let Err(error) = writer.write(final_part.clone()).await
    {
        let _ = writer.abort().await;
        return Err(error).with_context(|| format!("failed to upload final part for {path:?}"));
    }

    if let Err(error) = writer.close().await {
        let _ = writer.abort().await;
        return Err(error)
            .with_context(|| format!("failed to complete multipart upload for {path:?}"));
    }

    Ok(())
}

async fn join_workers(workers: &mut JoinSet<()>, workload: &str) -> Result<()> {
    while let Some(result) = workers.join_next().await {
        result.with_context(|| format!("{workload} benchmark worker failed"))?;
    }
    Ok(())
}

fn deterministic_buffer(length: usize, seed: u64) -> Buffer {
    let mut state = seed;
    let mut bytes = vec![0_u8; length];
    for byte in &mut bytes {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }
    Buffer::from(bytes)
}

fn run_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("run-{timestamp}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_data_is_deterministic_and_not_zero_filled() {
        let first = deterministic_buffer(1024, 42);
        let second = deterministic_buffer(1024, 42);
        let first = first.to_vec();
        let second = second.to_vec();

        assert_eq!(first, second);
        assert!(first.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn object_has_fifty_one_full_parts_and_one_final_part() {
        assert_eq!(OBJECT_SIZE / MULTIPART_PART_SIZE, 51);
        assert_eq!(OBJECT_SIZE % MULTIPART_PART_SIZE, 2 * MIB);
    }
}
