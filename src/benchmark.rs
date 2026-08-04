use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use opendal::{Buffer, Operator};
use tokio::{sync::Semaphore, task::JoinSet, time::Instant as TokioInstant};

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
        let write = self.run_write(&run_prefix).await?;

        eprintln!(
            "running stat workload for {}s at concurrency {}...",
            self.config.stat_duration_seconds, self.config.stat_concurrency
        );
        let stat = self.run_stat(&source_paths[0]).await?;

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

    async fn prepare_read_sources(&self, paths: &[String]) -> Result<()> {
        let part_writes = Arc::new(Semaphore::new(self.config.write_concurrency));
        let mut uploads = JoinSet::new();

        for path in paths {
            let operator = self.operator.clone();
            let path = path.clone();
            let part_writes = Arc::clone(&part_writes);
            let full_part = self.full_part.clone();
            let final_part = self.final_part.clone();
            uploads.spawn(async move {
                upload_object(
                    &operator,
                    &path,
                    &part_writes,
                    &full_part,
                    &final_part,
                    /*deadline=*/ None,
                )
                .await
                .with_context(|| format!("failed to prepare read source object {path:?}"))?;
                Ok::<(), anyhow::Error>(())
            });
        }

        while let Some(result) = uploads.join_next().await {
            result.context("read source upload task failed")??;
        }
        Ok(())
    }

    async fn run_read(&self, paths: Vec<String>) -> Result<BenchmarkReport> {
        let recorder = Arc::new(MetricsRecorder::default());
        let paths = Arc::new(paths);
        let elapsed = Duration::from_secs(self.config.read_duration_seconds);
        let deadline = TokioInstant::now() + elapsed;
        let read_requests = Arc::new(Semaphore::new(self.config.read_concurrency));
        let mut workers = JoinSet::new();

        for worker_id in 0..self.config.read_concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let paths = Arc::clone(&paths);
            let read_requests = Arc::clone(&read_requests);
            let concurrency = self.config.read_concurrency as u64;

            workers.spawn(async move {
                let path = &paths[worker_id % paths.len()];
                let mut block = worker_id as u64;
                loop {
                    let offset = (block % (OBJECT_SIZE / READ_SIZE)) * READ_SIZE;
                    block += concurrency;
                    let Some(permit) =
                        before_deadline(deadline, Arc::clone(&read_requests).acquire_owned()).await
                    else {
                        break;
                    };
                    let Ok(permit) = permit else {
                        recorder.record_error(&anyhow!("read semaphore closed"));
                        break;
                    };
                    let operation_started = Instant::now();
                    let Some(result) = before_deadline(deadline, async {
                        operator
                            .read_with(path)
                            .range(offset..offset + READ_SIZE)
                            .await
                    })
                    .await
                    else {
                        break;
                    };
                    drop(permit);
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
                            recorder.record_success(operation_started.elapsed(), READ_SIZE);
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
        let elapsed = Duration::from_secs(self.config.write_duration_seconds);
        let deadline = TokioInstant::now() + elapsed;
        let part_writes = Arc::new(Semaphore::new(self.config.write_concurrency));
        let mut workers = JoinSet::new();

        for worker_id in 0..self.config.write_concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let run_prefix = Arc::clone(&run_prefix);
            let full_part = self.full_part.clone();
            let final_part = self.final_part.clone();
            let part_writes = Arc::clone(&part_writes);

            workers.spawn(async move {
                let mut sequence = 0_u64;

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
                        &part_writes,
                        &full_part,
                        &final_part,
                        Some(deadline),
                    )
                    .await;

                    match result {
                        Ok(true) => {
                            recorder.record_success(operation_started.elapsed(), OBJECT_SIZE);
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
        let elapsed = Duration::from_secs(self.config.stat_duration_seconds);
        let deadline = TokioInstant::now() + elapsed;
        let mut workers = JoinSet::new();

        for _ in 0..self.config.stat_concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let path = Arc::clone(&path);

            workers.spawn(async move {
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
                            recorder.record_success(operation_started.elapsed(), 0);
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
    part_writes: &Arc<Semaphore>,
    full_part: &Buffer,
    final_part: &Buffer,
    deadline: Option<TokioInstant>,
) -> Result<bool> {
    let Some(writer) = maybe_before_deadline(deadline, async {
        operator
            .writer_with(path)
            .chunk(MULTIPART_PART_SIZE as usize)
            .await
    })
    .await
    else {
        return Ok(false);
    };
    let mut writer =
        writer.with_context(|| format!("failed to start multipart upload for {path:?}"))?;

    for _ in 0..(OBJECT_SIZE / MULTIPART_PART_SIZE) {
        let Some(permit) =
            maybe_before_deadline(deadline, Arc::clone(part_writes).acquire_owned()).await
        else {
            writer
                .abort()
                .await
                .with_context(|| format!("failed to abort timed-out upload for {path:?}"))?;
            return Ok(false);
        };
        let permit = permit.context("part-write semaphore closed")?;
        let result = writer.write(full_part.clone()).await;
        drop(permit);
        if let Err(error) = result {
            let _ = writer.abort().await;
            return Err(error).with_context(|| format!("failed to upload part for {path:?}"));
        }
    }

    if !final_part.is_empty() {
        let Some(permit) =
            maybe_before_deadline(deadline, Arc::clone(part_writes).acquire_owned()).await
        else {
            writer
                .abort()
                .await
                .with_context(|| format!("failed to abort timed-out upload for {path:?}"))?;
            return Ok(false);
        };
        let permit = permit.context("part-write semaphore closed")?;
        let result = writer.write(final_part.clone()).await;
        drop(permit);
        if let Err(error) = result {
            let _ = writer.abort().await;
            return Err(error).with_context(|| format!("failed to upload final part for {path:?}"));
        }
    }

    let result = writer.close().await;
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
