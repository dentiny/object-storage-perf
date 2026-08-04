use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use hdrhistogram::Histogram;

#[derive(Debug)]
pub struct BenchmarkReport {
    pub workload: &'static str,
    pub elapsed: Duration,
    pub successes: u64,
    pub errors: u64,
    pub bytes: u64,
    pub mean_latency_nanos: f64,
    pub p50_latency_nanos: u64,
    pub p95_latency_nanos: u64,
    pub p99_latency_nanos: u64,
    pub error_categories: Vec<(String, u64)>,
    pub first_error: Option<String>,
}

impl BenchmarkReport {
    pub fn average_latency_ms(&self) -> f64 {
        self.mean_latency_nanos / 1_000_000.0
    }

    pub fn operations_per_second(&self) -> f64 {
        self.successes as f64 / self.elapsed.as_secs_f64()
    }

    pub fn throughput_mib_per_second(&self) -> Option<f64> {
        (self.bytes > 0).then(|| self.bytes as f64 / (1024.0 * 1024.0) / self.elapsed.as_secs_f64())
    }

    pub fn success_rate(&self) -> f64 {
        let attempts = self.successes + self.errors;
        if attempts == 0 {
            0.0
        } else {
            self.successes as f64 / attempts as f64
        }
    }

    fn percentile_latency_ms(&self, latency_nanos: u64) -> f64 {
        latency_nanos as f64 / 1_000_000.0
    }
}

impl fmt::Display for BenchmarkReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:<5} latency avg/p50/p95/p99: {:.3}/{:.3}/{:.3}/{:.3} ms | ",
            self.workload,
            self.average_latency_ms(),
            self.percentile_latency_ms(self.p50_latency_nanos),
            self.percentile_latency_ms(self.p95_latency_nanos),
            self.percentile_latency_ms(self.p99_latency_nanos),
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
            "successes: {} | errors: {} | success rate: {:.2}%",
            self.successes,
            self.errors,
            self.success_rate() * 100.0,
        )?;

        if !self.error_categories.is_empty() {
            let categories = self
                .error_categories
                .iter()
                .map(|(category, count)| format!("{category}={count}"))
                .collect::<Vec<_>>()
                .join(",");
            write!(formatter, " | error categories: {categories}")?;
        }

        if let Some(error) = &self.first_error {
            write!(formatter, " | first error: {error}")?;
        }

        Ok(())
    }
}

pub(crate) struct MetricsRecorder {
    successes: AtomicU64,
    errors: AtomicU64,
    bytes: AtomicU64,
    latency: Mutex<Histogram<u64>>,
    error_categories: Mutex<BTreeMap<String, u64>>,
    first_error: Mutex<Option<String>>,
}

impl Default for MetricsRecorder {
    fn default() -> Self {
        Self {
            successes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            latency: Mutex::new(Histogram::<u64>::new(3).expect("valid histogram precision")),
            error_categories: Mutex::new(BTreeMap::new()),
            first_error: Mutex::new(None),
        }
    }
}

impl MetricsRecorder {
    pub(crate) fn record_success(&self, duration: Duration, bytes: u64) {
        self.successes.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        self.latency
            .lock()
            .unwrap()
            .saturating_record(duration_nanos(duration).max(1));
    }

    pub(crate) fn record_error(&self, error: &anyhow::Error) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        let category = error_category(error);
        *self
            .error_categories
            .lock()
            .unwrap()
            .entry(category)
            .or_default() += 1;
        let mut first_error = self.first_error.lock().unwrap();
        if first_error.is_none() {
            *first_error = Some(format!("{error:#}"));
        }
    }

    pub(crate) fn report(&self, workload: &'static str, elapsed: Duration) -> BenchmarkReport {
        let latency = self.latency.lock().unwrap();

        BenchmarkReport {
            workload,
            elapsed,
            successes: self.successes.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            mean_latency_nanos: latency.mean(),
            p50_latency_nanos: latency.value_at_quantile(0.50),
            p95_latency_nanos: latency.value_at_quantile(0.95),
            p99_latency_nanos: latency.value_at_quantile(0.99),
            error_categories: self
                .error_categories
                .lock()
                .unwrap()
                .iter()
                .map(|(category, count)| (category.clone(), *count))
                .collect(),
            first_error: self.first_error.lock().unwrap().clone(),
        }
    }
}

fn duration_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

fn error_category(error: &anyhow::Error) -> String {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<opendal::Error>())
        .map(|error| format!("{:?}", error.kind()))
        .unwrap_or_else(|| "Other".to_owned())
}
