use std::future::Future;

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

pub(crate) async fn join_workers(workers: &mut JoinSet<()>, workload: &str) -> Result<()> {
    while let Some(result) = workers.join_next().await {
        result.with_context(|| format!("{workload} benchmark worker failed"))?;
    }
    Ok(())
}

pub(crate) fn zero_buffer(length: usize) -> Buffer {
    Buffer::from(vec![0_u8; length])
}
