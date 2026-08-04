use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use tokio::{sync::Semaphore, task::JoinSet};

use super::{BenchmarkSuite, OBJECT_SIZE, READ_SIZE, write::upload_object};
use crate::{
    metrics::{BenchmarkReport, MetricsRecorder},
    utils::{MeasurementWindow, before_deadline, join_workers},
};

impl BenchmarkSuite {
    pub(super) async fn prepare_read_sources(&self, paths: &[String]) -> Result<()> {
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
                    /*metrics=*/ None,
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

    pub(super) async fn run_read(&self, paths: Vec<String>) -> Result<BenchmarkReport> {
        let recorder = Arc::new(MetricsRecorder::default());
        let paths = Arc::new(paths);
        let measurement = Arc::new(MeasurementWindow::new(Duration::from_secs(
            self.config.read_duration_seconds,
        )));
        let deadline = measurement.deadline();
        let read_requests = Arc::new(Semaphore::new(self.config.read_concurrency));
        let mut workers = JoinSet::new();

        for worker_id in 0..self.config.read_concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let paths = Arc::clone(&paths);
            let read_requests = Arc::clone(&read_requests);
            let measurement = Arc::clone(&measurement);
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
                    let result = operator
                        .read_with(path)
                        .range(offset..offset + READ_SIZE)
                        .await;
                    measurement.record_completion();
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
        Ok(recorder.report("read", measurement.elapsed()))
    }
}
