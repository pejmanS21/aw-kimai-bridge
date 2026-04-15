use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Persisted state written to disk after every successful sync cycle.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct State {
    /// The end-timestamp of the last event we successfully pushed to Kimai.
    /// On the next cycle we query AW for events strictly after this point.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// How many entries we've pushed in total (for diagnostics)
    pub total_entries_pushed: u64,
    /// ISO-8601 timestamp of when the daemon first started (informational)
    pub daemon_started_at: Option<DateTime<Utc>>,
}

pub struct StateManager {
    path: PathBuf,
    pub state: State,
}

impl StateManager {
    /// Load state from disk, or create a fresh default if the file doesn't exist.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        let state = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read state file: {}", path.display()))?;
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse state file: {}", path.display()))?
        } else {
            tracing::info!(path = %path.display(), "No state file found, starting fresh");
            State {
                daemon_started_at: Some(Utc::now()),
                ..Default::default()
            }
        };

        Ok(Self { path, state })
    }

    /// Persist the current state to disk atomically (write-then-rename).
    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.state)
            .context("Failed to serialize state")?;

        // Write to a temp file alongside the real one, then rename.
        // This prevents a corrupt state file if the process is killed mid-write.
        let tmp_path = self.path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json)
            .with_context(|| format!("Failed to write temp state file: {}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, &self.path)
            .with_context(|| format!("Failed to rename state file to: {}", self.path.display()))?;

        tracing::debug!(last_synced_at = ?self.state.last_synced_at, "State saved");
        Ok(())
    }

    /// Update cursor after a successful batch push.
    pub fn record_sync(&mut self, synced_until: DateTime<Utc>, entries_pushed: u64) -> Result<()> {
        self.state.last_synced_at = Some(synced_until);
        self.state.total_entries_pushed += entries_pushed;
        self.save()
    }
}
