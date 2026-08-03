use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use opendal::{Buffer, Operator};
use tokio::{task::JoinSet, time::Instant as TokioInstant};

use crate::{
    config::BenchmarkConfig,
    metrics::{BenchmarkReport, MetricsRecorder},
    storage::Storage,
    utils::{before_deadline, join_workers, maybe_before_deadline, zero_buffer},
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
            full_part: zero_buffer(MULTIPART_PART_SIZE as usize),
            final_part: zero_buffer((OBJECT_SIZE % MULTIPART_PART_SIZE) as usize),
        }
    }

    pub async fn run(&self) -> Result<Vec<BenchmarkReport>> {
        let run_prefix = format!("{}/{}", self.prefix, run_id());
        let source_path = format!("{run_prefix}/read-source");

        eprintln!("preparing 512 MiB source object {source_path:?}...");
        upload_object(
            &self.operator,
            &source_path,
            self.config.multipart_concurrency,
            &self.full_part,
            &self.final_part,
            /*deadline=*/ None,
        )
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
        let write = self.run_write(&run_prefix).await?;

        eprintln!(
            "running stat workload for {}s at concurrency {}...",
            self.config.duration_seconds, self.config.concurrency
        );
        let stat = self.run_stat(&source_path).await?;

        if self.config.keep_objects {
            eprintln!("retaining benchmark objects under {run_prefix:?}");
        } else {
            eprintln!("cleaning up benchmark objects under {run_prefix:?}...");
            if let Err(error) = self.cleanup(&run_prefix).await {
                eprintln!("warning: benchmark completed but cleanup failed: {error:#}");
            }
        }

        Ok(vec![read, write, stat])
    }

    async fn run_read(&self, path: &str) -> Result<BenchmarkReport> {
        let recorder = Arc::new(MetricsRecorder::default());
        let path = Arc::new(path.to_owned());
        let elapsed = Duration::from_secs(self.config.duration_seconds);
        let deadline = TokioInstant::now() + elapsed;
        let mut workers = JoinSet::new();

        for worker_id in 0..self.config.concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let path = Arc::clone(&path);
            let concurrency = self.config.concurrency as u64;

            workers.spawn(async move {
                let mut block = worker_id as u64;
                let mut latency = recorder.latency_recorder();
                loop {
                    let offset = (block % (OBJECT_SIZE / READ_SIZE)) * READ_SIZE;
                    block += concurrency;
                    let operation_started = Instant::now();
                    let Some(result) = before_deadline(deadline, async {
                        operator
                            .read_with(path.as_str())
                            .range(offset..offset + READ_SIZE)
                            .await
                    })
                    .await
                    else {
                        break;
                    };
                    let result = result.map_err(anyhow::Error::from).and_then(|buffer| {
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
                            recorder.record_success(
                                &mut latency,
                                operation_started.elapsed(),
                                READ_SIZE,
                            );
                        }
                        Err(error) => recorder.record_error(&error),
                    }
                }
            });
        }

        join_workers(&mut workers, "read").await?;
        Ok(recorder.report("read", elapsed))
    }

    async fn run_write(&self, run_prefix: &str) -> Result<BenchmarkReport> {
        let recorder = Arc::new(MetricsRecorder::default());
        let run_prefix = Arc::new(run_prefix.to_owned());
        let elapsed = Duration::from_secs(self.config.duration_seconds);
        let deadline = TokioInstant::now() + elapsed;
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
                let mut latency = recorder.latency_recorder();

                loop {
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
                        Some(deadline),
                    )
                    .await;

                    match result {
                        Ok(true) => {
                            recorder.record_success(
                                &mut latency,
                                operation_started.elapsed(),
                                OBJECT_SIZE,
                            );
                        }
                        Ok(false) => break,
                        Err(error) => recorder.record_error(&error),
                    }
                }
            });
        }

        join_workers(&mut workers, "write").await?;
        Ok(recorder.report("write", elapsed))
    }

    async fn run_stat(&self, path: &str) -> Result<BenchmarkReport> {
        let recorder = Arc::new(MetricsRecorder::default());
        let path = Arc::new(path.to_owned());
        let elapsed = Duration::from_secs(self.config.duration_seconds);
        let deadline = TokioInstant::now() + elapsed;
        let mut workers = JoinSet::new();

        for _ in 0..self.config.concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let path = Arc::clone(&path);

            workers.spawn(async move {
                let mut latency = recorder.latency_recorder();
                loop {
                    let operation_started = Instant::now();
                    let Some(result) =
                        before_deadline(deadline, operator.stat(path.as_str())).await
                    else {
                        break;
                    };
                    let result = result.map_err(anyhow::Error::from).and_then(|metadata| {
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
                        Ok(()) => {
                            recorder.record_success(&mut latency, operation_started.elapsed(), 0);
                        }
                        Err(error) => recorder.record_error(&error),
                    }
                }
            });
        }

        join_workers(&mut workers, "stat").await?;
        Ok(recorder.report("stat", elapsed))
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

async fn upload_object(
    operator: &Operator,
    path: &str,
    multipart_concurrency: usize,
    full_part: &Buffer,
    final_part: &Buffer,
    deadline: Option<TokioInstant>,
) -> Result<bool> {
    let Some(writer) = maybe_before_deadline(deadline, async {
        operator
            .writer_with(path)
            .chunk(MULTIPART_PART_SIZE as usize)
            .concurrent(multipart_concurrency)
            .await
    })
    .await
    else {
        return Ok(false);
    };
    let mut writer =
        writer.with_context(|| format!("failed to start multipart upload for {path:?}"))?;

    for _ in 0..(OBJECT_SIZE / MULTIPART_PART_SIZE) {
        let Some(result) = maybe_before_deadline(deadline, writer.write(full_part.clone())).await
        else {
            writer
                .abort()
                .await
                .with_context(|| format!("failed to abort timed-out upload for {path:?}"))?;
            return Ok(false);
        };
        if let Err(error) = result {
            let _ = writer.abort().await;
            return Err(error).with_context(|| format!("failed to upload part for {path:?}"));
        }
    }

    if !final_part.is_empty() {
        let Some(result) = maybe_before_deadline(deadline, writer.write(final_part.clone())).await
        else {
            writer
                .abort()
                .await
                .with_context(|| format!("failed to abort timed-out upload for {path:?}"))?;
            return Ok(false);
        };
        if let Err(error) = result {
            let _ = writer.abort().await;
            return Err(error).with_context(|| format!("failed to upload final part for {path:?}"));
        }
    }

    let Some(result) = maybe_before_deadline(deadline, writer.close()).await else {
        writer
            .abort()
            .await
            .with_context(|| format!("failed to abort timed-out upload for {path:?}"))?;
        return Ok(false);
    };
    if let Err(error) = result {
        let _ = writer.abort().await;
        return Err(error)
            .with_context(|| format!("failed to complete multipart upload for {path:?}"));
    }

    Ok(true)
}

fn run_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("run-{timestamp}-{}", std::process::id())
}
