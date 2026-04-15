use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;

/// Raw event as returned by the ActivityWatch /api/0/buckets/{id}/events endpoint.
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct AwEvent {
    pub id: Option<u64>,
    pub timestamp: DateTime<Utc>,
    /// Duration in fractional seconds (AW's native unit)
    pub duration: f64,
    pub data: AwEventData,
}

impl AwEvent {
    /// Duration as whole seconds, floored.
    #[allow(dead_code)]
    pub fn duration_secs(&self) -> i64 {
        self.duration.floor() as i64
    }

    /// Inclusive end timestamp.
    pub fn end(&self) -> DateTime<Utc> {
        self.timestamp + chrono::Duration::milliseconds((self.duration * 1000.0) as i64)
    }
}

/// The `data` payload of a window-watcher event.
#[derive(Debug, Deserialize, Clone)]
pub struct AwEventData {
    /// Active window title at the time of the event
    pub title: String,
    /// Executable / app name
    #[serde(default)]
    pub app: String,
    /// URL (populated by browser watchers)
    #[serde(default)]
    pub url: Option<String>,
}

/// Metadata for an ActivityWatch bucket.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct AwBucket {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub client: String,
    pub hostname: String,
}

pub struct AwClient {
    http: Client,
    base_url: String,
}

impl AwClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Fetch all events from a bucket between [start, end).
    /// If `since` is None, fetches all available events (use carefully on
    /// large buckets — the daemon always passes a cursor).
    pub async fn get_events(
        &self,
        bucket_id: &str,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> Result<Vec<AwEvent>> {
        let url = format!("{}/api/0/buckets/{}/events", self.base_url, bucket_id);

        let mut req = self.http.get(&url);

        if let Some(start) = since {
            req = req.query(&[("start", start.to_rfc3339())]);
        }
        if let Some(end) = until {
            req = req.query(&[("end", end.to_rfc3339())]);
        }
        if let Some(n) = limit {
            req = req.query(&[("limit", n.to_string())]);
        }

        let resp = req
            .send()
            .await
            .context("Failed to reach ActivityWatch API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "ActivityWatch returned HTTP {status} for bucket {bucket_id}: {body}"
            );
        }

        let events: Vec<AwEvent> = resp
            .json()
            .await
            .context("Failed to deserialize ActivityWatch events")?;

        tracing::debug!(
            bucket = bucket_id,
            count = events.len(),
            "Fetched events from ActivityWatch"
        );

        Ok(events)
    }

    /// List all buckets registered with this ActivityWatch instance.
    #[allow(dead_code)]
    pub async fn list_buckets(&self) -> Result<Vec<AwBucket>> {
        let url = format!("{}/api/0/buckets/", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("Failed to list ActivityWatch buckets")?;

        // AW returns a map keyed by bucket ID; collect into a Vec.
        let map: std::collections::HashMap<String, AwBucket> = resp
            .json()
            .await
            .context("Failed to deserialize bucket list")?;

        Ok(map.into_values().collect())
    }

    /// Verify that the ActivityWatch daemon is reachable.
    pub async fn ping(&self) -> Result<()> {
        let url = format!("{}/api/0/info", self.base_url);
        self.http
            .get(&url)
            .send()
            .await
            .context("ActivityWatch is not reachable")?;
        Ok(())
    }
}
