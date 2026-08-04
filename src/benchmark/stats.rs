use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use tokio::{task::JoinSet, time::Instant as TokioInstant};

use super::{BenchmarkSuite, OBJECT_SIZE};
use crate::{
    metrics::{BenchmarkReport, MetricsRecorder},
    utils::{MeasurementWindow, join_workers},
};

impl BenchmarkSuite {
    pub(super) async fn run_stat(&self, path: &str) -> Result<BenchmarkReport> {
        let recorder = Arc::new(MetricsRecorder::default());
        let path = Arc::new(path.to_owned());
        let measurement = Arc::new(MeasurementWindow::new(Duration::from_secs(
            self.config.stat_duration_seconds,
        )));
        let deadline = measurement.deadline();
        let mut workers = JoinSet::new();

        for _ in 0..self.config.stat_concurrency {
            let operator = self.operator.clone();
            let recorder = Arc::clone(&recorder);
            let path = Arc::clone(&path);
            let measurement = Arc::clone(&measurement);

            workers.spawn(async move {
                loop {
                    if TokioInstant::now() >= deadline {
                        break;
                    }
                    let operation_started = Instant::now();
                    let result = operator.stat(path.as_str()).await;
                    measurement.record_completion();
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
        Ok(recorder.report("stat", measurement.elapsed()))
    }
}
