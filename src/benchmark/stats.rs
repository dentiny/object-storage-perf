use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use tokio::{task::JoinSet, time::Instant as TokioInstant};

use super::{BenchmarkSuite, OBJECT_SIZE};
use crate::{
    metrics::{BenchmarkReport, MetricsRecorder},
    utils::{before_deadline, join_workers},
};

impl BenchmarkSuite {
    pub(super) async fn run_stat(&self, path: &str) -> Result<BenchmarkReport> {
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
}
