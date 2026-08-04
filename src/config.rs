use anyhow::{Result, bail};
use clap::Args;

const DEFAULT_REGION: &str = "us-east-1";
const DEFAULT_PREFIX: &str = "object-storage-perf";
const DEFAULT_DURATION_SECONDS: u64 = 10;
const DEFAULT_READ_CONCURRENCY: usize = 64;
const DEFAULT_WRITE_CONCURRENCY: usize = 128;
const DEFAULT_STAT_CONCURRENCY: usize = 4;
const DEFAULT_RETRY_MAX_TIMES: usize = 3;
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

#[derive(Args, Clone)]
pub struct StorageArgs {
    /// S3-compatible API endpoint, including http:// or https://.
    #[arg(long, env = "OSP_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Existing bucket used by the benchmark.
    #[arg(long, env = "OSP_BUCKET")]
    pub bucket: Option<String>,

    /// Signing region used by the S3-compatible API.
    #[arg(
        long,
        env = "OSP_REGION",
        default_value = DEFAULT_REGION
    )]
    pub region: String,

    /// S3 access key ID.
    #[arg(long, env = "OSP_ACCESS_KEY_ID", hide_env_values = true)]
    pub access_key_id: Option<String>,

    /// S3 secret access key.
    #[arg(long, env = "OSP_SECRET_ACCESS_KEY", hide_env_values = true)]
    pub secret_access_key: Option<String>,

    /// Run-scoped prefix used for benchmark-owned objects.
    #[arg(
        long,
        env = "OSP_PREFIX",
        default_value = DEFAULT_PREFIX
    )]
    pub prefix: String,

    /// Maximum retry attempts for temporary storage errors.
    #[arg(
        long,
        env = "OSP_RETRY_MAX_TIMES",
        default_value_t = DEFAULT_RETRY_MAX_TIMES
    )]
    pub retry_max_times: usize,

    /// Timeout for non-I/O storage operations such as stat.
    #[arg(
        long,
        env = "OSP_TIMEOUT_SECONDS",
        default_value_t = DEFAULT_TIMEOUT_SECONDS
    )]
    pub timeout_seconds: u64,

    /// Timeout for each storage I/O attempt.
    #[arg(
        long,
        env = "OSP_IO_TIMEOUT_SECONDS",
        default_value_t = DEFAULT_TIMEOUT_SECONDS
    )]
    pub io_timeout_seconds: u64,
}

#[derive(Args, Clone)]
pub struct BenchmarkArgs {
    /// Measured duration of the read workload.
    #[arg(
        long,
        env = "OSP_READ_DURATION",
        default_value_t = DEFAULT_DURATION_SECONDS
    )]
    pub read_duration_seconds: u64,

    /// Measured duration of the write workload.
    #[arg(
        long,
        env = "OSP_WRITE_DURATION",
        default_value_t = DEFAULT_DURATION_SECONDS
    )]
    pub write_duration_seconds: u64,

    /// Measured duration of the stat workload.
    #[arg(
        long,
        env = "OSP_STAT_DURATION",
        default_value_t = DEFAULT_DURATION_SECONDS
    )]
    pub stat_duration_seconds: u64,

    /// Maximum number of in-flight range-read requests.
    #[arg(
        long,
        env = "OSP_READ_CONCURRENCY",
        default_value_t = DEFAULT_READ_CONCURRENCY
    )]
    pub read_concurrency: usize,

    /// Maximum number of in-flight multipart part-write requests.
    #[arg(
        long,
        env = "OSP_WRITE_CONCURRENCY",
        default_value_t = DEFAULT_WRITE_CONCURRENCY
    )]
    pub write_concurrency: usize,

    /// Maximum number of in-flight stat requests.
    #[arg(
        long,
        env = "OSP_STAT_CONCURRENCY",
        default_value_t = DEFAULT_STAT_CONCURRENCY
    )]
    pub stat_concurrency: usize,

    /// Retain all objects created by the benchmark.
    #[arg(long, env = "OSP_KEEP_OBJECTS", default_value_t = false)]
    pub keep_objects: bool,
}

pub struct StorageConfig {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub prefix: String,
    pub retry_max_times: usize,
    pub timeout_seconds: u64,
    pub io_timeout_seconds: u64,
}

pub struct BenchmarkConfig {
    pub read_duration_seconds: u64,
    pub write_duration_seconds: u64,
    pub stat_duration_seconds: u64,
    pub read_concurrency: usize,
    pub write_concurrency: usize,
    pub stat_concurrency: usize,
    pub keep_objects: bool,
}

impl TryFrom<StorageArgs> for StorageConfig {
    type Error = anyhow::Error;

    fn try_from(args: StorageArgs) -> Result<Self> {
        let endpoint = required_option("endpoint", args.endpoint)?;
        let endpoint = endpoint.trim_end_matches('/').to_owned();
        if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
            bail!("endpoint must start with http:// or https://");
        }

        let bucket = required_option("bucket", args.bucket)?;
        let region = required("region", args.region)?;
        let access_key_id = required_option("access key ID", args.access_key_id)?;
        let secret_access_key = required_option("secret access key", args.secret_access_key)?;
        let prefix = args.prefix.trim().trim_matches('/').to_owned();
        if prefix.is_empty() {
            bail!("prefix cannot be empty");
        }
        if args.retry_max_times == 0 {
            bail!("retry max times must be greater than zero");
        }
        if args.timeout_seconds == 0 || args.io_timeout_seconds == 0 {
            bail!("storage timeouts must be greater than zero");
        }

        Ok(Self {
            endpoint,
            bucket,
            region,
            access_key_id,
            secret_access_key,
            prefix,
            retry_max_times: args.retry_max_times,
            timeout_seconds: args.timeout_seconds,
            io_timeout_seconds: args.io_timeout_seconds,
        })
    }
}

impl TryFrom<BenchmarkArgs> for BenchmarkConfig {
    type Error = anyhow::Error;

    fn try_from(args: BenchmarkArgs) -> Result<Self> {
        if args.read_duration_seconds == 0
            || args.write_duration_seconds == 0
            || args.stat_duration_seconds == 0
        {
            bail!("workload durations must be greater than zero");
        }
        if args.read_concurrency == 0 || args.write_concurrency == 0 || args.stat_concurrency == 0 {
            bail!("workload concurrency values must be greater than zero");
        }

        Ok(Self {
            read_duration_seconds: args.read_duration_seconds,
            write_duration_seconds: args.write_duration_seconds,
            stat_duration_seconds: args.stat_duration_seconds,
            read_concurrency: args.read_concurrency,
            write_concurrency: args.write_concurrency,
            stat_concurrency: args.stat_concurrency,
            keep_objects: args.keep_objects,
        })
    }
}

fn required(name: &str, value: String) -> Result<String> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        bail!("{name} cannot be empty");
    }
    Ok(value)
}

fn required_option(name: &str, value: Option<String>) -> Result<String> {
    let value = value.ok_or_else(|| anyhow::anyhow!("{name} is required"))?;
    required(name, value)
}
