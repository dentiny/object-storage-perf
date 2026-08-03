use anyhow::{Context, Result, bail};
use futures_util::TryStreamExt;
use opendal::{Operator, services};

use crate::config::StorageConfig;

pub struct Storage {
    operator: Operator,
    prefix: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum CheckResult {
    Object {
        path: String,
        content_length: u64,
    },
    Prefix {
        prefix: String,
        first_entry: Option<String>,
    },
}

impl Storage {
    pub fn new(config: StorageConfig) -> Result<Self> {
        let builder = services::S3::default()
            .bucket(&config.bucket)
            .endpoint(&config.endpoint)
            .region(&config.region)
            .access_key_id(&config.access_key_id)
            .secret_access_key(&config.secret_access_key)
            .disable_config_load();

        let operator = Operator::new(builder).context("failed to configure S3 operator")?;
        validate_capabilities(&operator)?;

        Ok(Self {
            operator,
            prefix: config.prefix,
        })
    }

    pub fn operator(&self) -> &Operator {
        &self.operator
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub async fn check(&self, object: Option<&str>) -> Result<CheckResult> {
        match object {
            Some(path) => {
                let path = normalize_object_path(path)?;
                let metadata = self
                    .operator
                    .stat(&path)
                    .await
                    .with_context(|| format!("failed to stat object {path:?}"))?;

                Ok(CheckResult::Object {
                    path,
                    content_length: metadata.content_length(),
                })
            }
            None => {
                let prefix = format!("{}/", self.prefix);
                let mut lister = self
                    .operator
                    .lister_with(&prefix)
                    .limit(1)
                    .await
                    .with_context(|| format!("failed to list benchmark prefix {prefix:?}"))?;
                let first_entry = lister
                    .try_next()
                    .await
                    .with_context(|| format!("failed to list benchmark prefix {prefix:?}"))?
                    .map(|entry| entry.path().to_owned());

                Ok(CheckResult::Prefix {
                    prefix,
                    first_entry,
                })
            }
        }
    }
}

fn validate_capabilities(operator: &Operator) -> Result<()> {
    let capabilities = operator.info().capability();
    let mut missing = Vec::new();

    if !capabilities.stat {
        missing.push("stat");
    }
    if !capabilities.read {
        missing.push("read");
    }
    if !capabilities.write {
        missing.push("write");
    }
    if !capabilities.list {
        missing.push("list");
    }

    if !missing.is_empty() {
        bail!(
            "configured storage is missing required capabilities: {}",
            missing.join(", ")
        );
    }

    Ok(())
}

fn normalize_object_path(path: &str) -> Result<String> {
    let path = path.trim().trim_start_matches('/').to_owned();
    if path.is_empty() || path.ends_with('/') {
        bail!("object path must name a file");
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_object_path() {
        assert_eq!(
            normalize_object_path(" /existing/object ").unwrap(),
            "existing/object"
        );
    }

    #[test]
    fn rejects_directory_object_path() {
        assert!(normalize_object_path("existing/").is_err());
    }
}
