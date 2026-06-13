use crate::merger::TimeBlock;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Payload sent to POST /api/timesheets
#[derive(Debug, Serialize)]
struct CreateTimesheetRequest {
    begin: String,      // ISO-8601
    end: String,        // ISO-8601
    project: u32,
    activity: u32,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<u32>,
}

/// Minimal representation of a Kimai timesheet entry
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct TimesheetEntry {
    pub id: u64,
    pub begin: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub duration: i64,
    pub project: u32,
    pub activity: u32,
}

pub struct KimaiClient {
    http: Client,
    base_url: String,
    token: String,
    user_id: Option<u32>,
}

impl KimaiClient {
    pub fn new(base_url: &str, token: &str, user_id: Option<u32>) -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
            user_id,
        }
    }

    /// Push a single merged TimeBlock to Kimai as a timesheet entry.
    pub async fn push_entry(&self, block: &TimeBlock) -> Result<TimesheetEntry> {
        let url = format!("{}/api/timesheets", self.base_url);

        // ISO-8601 with explicit timezone offset. Without a tz suffix Kimai
        // interprets the timestamp as the server's local time, which silently
        // shifts entries by the offset between UTC and the server's tz.
        let body = CreateTimesheetRequest {
            begin: block.start.format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            end: block.end.format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            project: block.project_id,
            activity: block.activity_id,
            description: truncate(&block.description, 255),
            user: self.user_id,
        };

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .context("Failed to reach Kimai API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Kimai returned HTTP {status}: {body}");
        }

        let entry: TimesheetEntry = resp
            .json()
            .await
            .context("Failed to deserialize Kimai response")?;

        tracing::info!(
            kimai_id = entry.id,
            project = block.project_id,
            duration_secs = block.duration_secs(),
            description = %block.description,
            "Entry pushed to Kimai"
        );

        Ok(entry)
    }

    /// Push a batch of TimeBlocks, returning one Result per block in the same
    /// order. Errors are non-fatal — callers decide what to do with the
    /// per-block outcomes (e.g. advance the cursor only up to the first
    /// failure so the rest get retried next cycle).
    pub async fn push_batch(
        &self,
        blocks: &[TimeBlock],
    ) -> Vec<Result<TimesheetEntry>> {
        let mut results = Vec::with_capacity(blocks.len());
        for block in blocks {
            let r = self.push_entry(block).await;
            if let Err(e) = &r {
                tracing::warn!(
                    project = block.project_id,
                    start = %block.start,
                    error = %e,
                    "Failed to push entry, will retry next cycle"
                );
            }
            results.push(r);
        }
        results
    }

    /// List recent timesheet entries for the configured user.
    /// Used during startup to check for duplicates if the cursor is lost.
    #[allow(dead_code)]
    #[allow(dead_code)]
    pub async fn list_recent_entries(
        &self,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<TimesheetEntry>> {
        let url = format!("{}/api/timesheets", self.base_url);

        let mut req = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .query(&[("size", limit.to_string())]);

        if let Some(start) = since {
            req = req.query(&[("begin", start.format("%Y-%m-%dT%H:%M:%S%:z").to_string())]);
        }

        let resp = req.send().await.context("Failed to list Kimai entries")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Kimai list request failed HTTP {status}: {body}");
        }

        let entries: Vec<TimesheetEntry> = resp
            .json()
            .await
            .context("Failed to deserialize Kimai entry list")?;

        Ok(entries)
    }

    /// Confirm the Kimai API is reachable and the token is valid.
    pub async fn ping(&self) -> Result<()> {
        let url = format!("{}/api/version", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to reach Kimai API")?;

        if resp.status() == 401 {
            anyhow::bail!("Kimai API token is invalid or expired");
        }
        if !resp.status().is_success() {
            anyhow::bail!("Kimai ping returned HTTP {}", resp.status());
        }
        Ok(())
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars - 1).collect();
        out.push('…');
        out
    }
}
