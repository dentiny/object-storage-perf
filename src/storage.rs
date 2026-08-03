use anyhow::{Context, Result, bail};
use opendal::{Operator, services};

use crate::config::StorageConfig;

#[derive(Clone)]
pub struct Storage {
    operator: Operator,
    prefix: String,
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
    if !missing.is_empty() {
        bail!(
            "configured storage is missing required capabilities: {}",
            missing.join(", ")
        );
    }

    Ok(())
}
