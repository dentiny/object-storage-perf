use anyhow::{Result, bail};
use clap::Args;

const DEFAULT_REGION: &str = "us-east-1";
const DEFAULT_PREFIX: &str = "object-storage-perf";

#[derive(Args, Clone)]
pub struct StorageArgs {
    /// S3-compatible API endpoint, including http:// or https://.
    #[arg(long, env = "OSP_ENDPOINT", global = true)]
    pub endpoint: Option<String>,

    /// Existing bucket used by the benchmark.
    #[arg(long, env = "OSP_BUCKET", global = true)]
    pub bucket: Option<String>,

    /// Signing region used by the S3-compatible API.
    #[arg(
        long,
        env = "OSP_REGION",
        default_value = DEFAULT_REGION,
        global = true
    )]
    pub region: String,

    /// S3 access key ID.
    #[arg(long, env = "OSP_ACCESS_KEY_ID", hide_env_values = true, global = true)]
    pub access_key_id: Option<String>,

    /// S3 secret access key.
    #[arg(
        long,
        env = "OSP_SECRET_ACCESS_KEY",
        hide_env_values = true,
        global = true
    )]
    pub secret_access_key: Option<String>,

    /// Run-scoped prefix used for benchmark-owned objects.
    #[arg(
        long,
        env = "OSP_PREFIX",
        default_value = DEFAULT_PREFIX,
        global = true
    )]
    pub prefix: String,
}

pub struct StorageConfig {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub prefix: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_args() -> StorageArgs {
        StorageArgs {
            endpoint: Some("https://storage.example.com/".to_owned()),
            bucket: Some("bench".to_owned()),
            region: "us-east-1".to_owned(),
            access_key_id: Some("access".to_owned()),
            secret_access_key: Some("secret".to_owned()),
            prefix: "/runs/".to_owned(),
        }
    }

    #[test]
    fn normalizes_endpoint_and_prefix() {
        let config = StorageConfig::try_from(valid_args()).unwrap();

        assert_eq!(config.endpoint, "https://storage.example.com");
        assert_eq!(config.prefix, "runs");
    }

    #[test]
    fn rejects_endpoint_without_scheme() {
        let mut args = valid_args();
        args.endpoint = Some("storage.example.com".to_owned());

        assert!(
            StorageConfig::try_from(args)
                .err()
                .unwrap()
                .to_string()
                .contains("http:// or https://")
        );
    }

    #[test]
    fn rejects_empty_prefix() {
        let mut args = valid_args();
        args.prefix = "///".to_owned();

        assert!(
            StorageConfig::try_from(args)
                .err()
                .unwrap()
                .to_string()
                .contains("prefix cannot be empty")
        );
    }
}
