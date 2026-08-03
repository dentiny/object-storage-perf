use std::{
    fmt,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

#[derive(Debug)]
pub struct BenchmarkReport {
    pub workload: &'static str,
    pub elapsed: Duration,
    pub successes: u64,
    pub errors: u64,
    pub bytes: u64,
    pub total_latency_nanos: u64,
    pub first_error: Option<String>,
}

impl BenchmarkReport {
    pub fn average_latency_ms(&self) -> f64 {
        if self.successes == 0 {
            return 0.0;
        }
        self.total_latency_nanos as f64 / self.successes as f64 / 1_000_000.0
    }

    pub fn operations_per_second(&self) -> f64 {
        self.successes as f64 / self.elapsed.as_secs_f64()
    }

    pub fn throughput_mib_per_second(&self) -> Option<f64> {
        (self.bytes > 0).then(|| self.bytes as f64 / (1024.0 * 1024.0) / self.elapsed.as_secs_f64())
    }
}

impl fmt::Display for BenchmarkReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:<5} average latency: {:>10.3} ms | ",
            self.workload,
            self.average_latency_ms()
        )?;

        if let Some(throughput) = self.throughput_mib_per_second() {
            write!(formatter, "throughput: {:>10.3} MiB/s | ", throughput)?;
        } else {
            write!(
                formatter,
                "throughput: {:>10.3} ops/s | ",
                self.operations_per_second()
            )?;
        }

        write!(
            formatter,
            "successes: {} | errors: {}",
            self.successes, self.errors
        )?;

        if let Some(error) = &self.first_error {
            write!(formatter, " | first error: {error}")?;
        }

        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct MetricsRecorder {
    successes: AtomicU64,
    errors: AtomicU64,
    bytes: AtomicU64,
    total_latency_nanos: AtomicU64,
    first_error: Mutex<Option<String>>,
}

impl MetricsRecorder {
    pub(crate) fn record_success(&self, latency: Duration, bytes: u64) {
        self.successes.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        self.total_latency_nanos
            .fetch_add(duration_nanos(latency), Ordering::Relaxed);
    }

    pub(crate) fn record_error(&self, error: &anyhow::Error) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        let mut first_error = self.first_error.lock().unwrap();
        if first_error.is_none() {
            *first_error = Some(format!("{error:#}"));
        }
    }

    pub(crate) fn report(&self, workload: &'static str, elapsed: Duration) -> BenchmarkReport {
        BenchmarkReport {
            workload,
            elapsed,
            successes: self.successes.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            total_latency_nanos: self.total_latency_nanos.load(Ordering::Relaxed),
            first_error: self.first_error.lock().unwrap().clone(),
        }
    }
}

fn duration_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}
