use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use anyhow::{Context, Result};
use opendal::Buffer;
use tokio::{
    task::JoinSet,
    time::{Instant, timeout_at},
};

pub(crate) async fn before_deadline<F, T>(deadline: Instant, future: F) -> Option<T>
where
    F: Future<Output = T>,
{
    timeout_at(deadline, future).await.ok()
}

pub(crate) async fn maybe_before_deadline<F, T>(deadline: Option<Instant>, future: F) -> Option<T>
where
    F: Future<Output = T>,
{
    match deadline {
        Some(deadline) => before_deadline(deadline, future).await,
        None => Some(future.await),
    }
}

pub(crate) struct MeasurementWindow {
    started: Instant,
    requested: Duration,
    last_completion_nanos: AtomicU64,
}

impl MeasurementWindow {
    pub(crate) fn new(requested: Duration) -> Self {
        Self {
            started: Instant::now(),
            requested,
            last_completion_nanos: AtomicU64::new(0),
        }
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.started + self.requested
    }

    pub(crate) fn record_completion(&self) {
        self.last_completion_nanos
            .fetch_max(duration_nanos(self.started.elapsed()), Ordering::SeqCst);
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.requested.max(Duration::from_nanos(
            self.last_completion_nanos.load(Ordering::SeqCst),
        ))
    }
}

pub(crate) async fn join_workers(workers: &mut JoinSet<()>, workload: &str) -> Result<()> {
    while let Some(result) = workers.join_next().await {
        result.with_context(|| format!("{workload} benchmark worker failed"))?;
    }
    Ok(())
}

pub(crate) fn zero_buffer(length: usize) -> Buffer {
    Buffer::from(vec![0_u8; length])
}

fn duration_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}
