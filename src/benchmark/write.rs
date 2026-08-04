use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use opendal::{Buffer, Operator};
use tokio::{sync::Semaphore, task::JoinSet, time::Instant as TokioInstant};

use super::{BenchmarkSuite, MULTIPART_PART_SIZE, OBJECT_SIZE};
use crate::{
    metrics::{BenchmarkReport, MetricsRecorder},
    utils::{MeasurementWindow, join_workers, maybe_before_deadline},
};

impl BenchmarkSuite {
    pub(super) async fn run_write(&self, run_prefix: &str) -> Result<BenchmarkReport> {
        let recorder = Arc::new(MetricsRecorder::default());
        let run_prefix = Arc::new(run_prefix.to_owned());
        let measurement = Arc::new(MeasurementWindow::new(Duration::from_secs(
            self.config.write_duration_seconds,
        )));
        let deadline = measurement.deadline();
        let part_writes = Arc::new(Semaphore::new(self.config.write_concurrency));
        let mut workers = JoinSet::new();

        for worker_id in 0..self.config.write_concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let run_prefix = Arc::clone(&run_prefix);
            let full_part = self.full_part.clone();
            let final_part = self.final_part.clone();
            let part_writes = Arc::clone(&part_writes);
            let measurement = Arc::clone(&measurement);

            workers.spawn(async move {
                let mut sequence = 0_u64;

                loop {
                    let path = format!(
                        "{}/writes/worker-{worker_id}/object-{sequence}",
                        run_prefix.as_str()
                    );
                    sequence += 1;
                    let result = upload_object(
                        &operator,
                        &path,
                        &part_writes,
                        &full_part,
                        &final_part,
                        Some(deadline),
                        Some((&recorder, &measurement)),
                    )
                    .await;

                    match result {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(error) => recorder.record_error(&error),
                    }
                }
            });
        }

        join_workers(&mut workers, "write").await?;
        Ok(recorder.report("write", measurement.elapsed()))
    }
}

pub(super) async fn upload_object(
    operator: &Operator,
    path: &str,
    part_writes: &Arc<Semaphore>,
    full_part: &Buffer,
    final_part: &Buffer,
    deadline: Option<TokioInstant>,
    metrics: Option<(&MetricsRecorder, &MeasurementWindow)>,
) -> Result<bool> {
    if deadline.is_some_and(|deadline| TokioInstant::now() >= deadline) {
        return Ok(false);
    }
    let writer = operator
        .writer_with(path)
        .chunk(MULTIPART_PART_SIZE as usize)
        .await;
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
        let operation_started = Instant::now();
        let result = writer.write(full_part.clone()).await;
        if let Some((recorder, measurement)) = metrics {
            measurement.record_completion();
            if result.is_ok() {
                recorder.record_success(operation_started.elapsed(), full_part.len() as u64);
            }
        }
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
        let operation_started = Instant::now();
        let result = writer.write(final_part.clone()).await;
        if let Some((recorder, measurement)) = metrics {
            measurement.record_completion();
            if result.is_ok() {
                recorder.record_success(operation_started.elapsed(), final_part.len() as u64);
            }
        }
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
