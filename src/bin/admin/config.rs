// The admin CLI re-uses the same config.toml format as the daemon.
// We only need the [kimai] section here.
// src/bin/admin/config.rs

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub kimai: KimaiConfig,
}

#[derive(Debug, Deserialize)]
pub struct KimaiConfig {
    pub url: String,
    pub token: String,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Config not found: {}", path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("Failed to parse config: {}", path.display()))
    }
}
