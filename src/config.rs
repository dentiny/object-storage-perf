use anyhow::{Result, bail};
use clap::Args;

const DEFAULT_REGION: &str = "us-east-1";
const DEFAULT_PREFIX: &str = "object-storage-perf";
const DEFAULT_DURATION_SECONDS: u64 = 10;
const DEFAULT_CONCURRENCY: usize = 4;
const DEFAULT_MULTIPART_CONCURRENCY: usize = 1;

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
}

#[derive(Args, Clone)]
pub struct BenchmarkArgs {
    /// Measured duration of each read, write, and stat workload.
    #[arg(long, env = "OSP_DURATION", default_value_t = DEFAULT_DURATION_SECONDS)]
    pub duration_seconds: u64,

    /// Number of concurrent logical operations.
    #[arg(long, env = "OSP_CONCURRENCY", default_value_t = DEFAULT_CONCURRENCY)]
    pub concurrency: usize,

    /// Concurrent multipart requests within each object upload.
    #[arg(
        long,
        env = "OSP_MULTIPART_CONCURRENCY",
        default_value_t = DEFAULT_MULTIPART_CONCURRENCY
    )]
    pub multipart_concurrency: usize,

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
}

pub struct BenchmarkConfig {
    pub duration_seconds: u64,
    pub concurrency: usize,
    pub multipart_concurrency: usize,
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

        Ok(Self {
            endpoint,
            bucket,
            region,
            access_key_id,
            secret_access_key,
            prefix,
        })
    }
}

impl TryFrom<BenchmarkArgs> for BenchmarkConfig {
    type Error = anyhow::Error;

    fn try_from(args: BenchmarkArgs) -> Result<Self> {
        if args.duration_seconds == 0 {
            bail!("duration must be greater than zero");
        }
        if args.concurrency == 0 {
            bail!("concurrency must be greater than zero");
        }
        if args.multipart_concurrency == 0 {
            bail!("multipart concurrency must be greater than zero");
        }

        Ok(Self {
            duration_seconds: args.duration_seconds,
            concurrency: args.concurrency,
            multipart_concurrency: args.multipart_concurrency,
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
