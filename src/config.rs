use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Top-level config loaded from config.toml
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub kimai: KimaiConfig,
    pub activitywatch: ActivityWatchConfig,

    /// How often (seconds) the daemon wakes up to sync
    #[serde(default = "default_interval")]
    pub sync_interval_secs: u64,

    /// Gaps shorter than this (seconds) between same-project events are
    /// merged into one continuous block (handles brief alt-tabs, etc.)
    #[serde(default = "default_idle_threshold")]
    pub idle_threshold_secs: i64,

    /// Minimum duration (seconds) for an event to be pushed to Kimai.
    /// Events shorter than this are silently dropped.
    #[serde(default = "default_min_duration")]
    pub min_duration_secs: i64,

    /// Where to persist the sync cursor (last synced timestamp).
    #[serde(default = "default_state_path")]
    pub state_path: PathBuf,

    /// Ordered list of classification rules. First match wins.
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct KimaiConfig {
    pub url: String,
    pub token: String,
    /// Fallback project ID when no rule matches
    pub default_project_id: u32,
    /// Fallback activity ID when no rule matches
    pub default_activity_id: u32,
    /// Kimai user ID to assign time entries to
    pub user_id: Option<u32>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ActivityWatchConfig {
    #[serde(default = "default_aw_url")]
    pub url: String,
    /// Name of the watcher bucket. Usually "aw-watcher-window_<hostname>".
    pub bucket: String,
}

/// A single classification rule mapping window titles to a Kimai project.
#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    /// Regex pattern matched against the window title (case-insensitive)
    pub pattern: String,
    pub project_id: u32,
    pub activity_id: u32,
    /// Optional human-readable label shown in logs
    pub label: Option<String>,
}

// ── defaults ──────────────────────────────────────────────────────────────────

fn default_interval() -> u64 {
    300 // 5 minutes
}

fn default_idle_threshold() -> i64 {
    120 // 2 minutes
}

fn default_min_duration() -> i64 {
    60 // 1 minute
}

fn default_state_path() -> PathBuf {
    PathBuf::from("state.json")
}

fn default_aw_url() -> String {
    "http://localhost:5600".to_string()
}

// ── loading ───────────────────────────────────────────────────────────────────

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path)
            .context(format!("Failed to read config file: {}. Run `aw-kimai-admin setup` to create one", path.display()))?;
        let config: Config = toml::from_str(&raw)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.kimai.url.is_empty() {
            anyhow::bail!("kimai.url must not be empty");
        }
        if self.kimai.token.is_empty() {
            anyhow::bail!("kimai.token must not be empty");
        }
        if self.activitywatch.bucket.is_empty() {
            anyhow::bail!("activitywatch.bucket must not be empty");
        }
        // Validate all regex patterns up-front so we fail fast at startup
        for rule in &self.rules {
            regex::Regex::new(&rule.pattern)
                .with_context(|| format!("Invalid regex pattern in rule: {:?}", rule.pattern))?;
        }
        Ok(())
    }
}
