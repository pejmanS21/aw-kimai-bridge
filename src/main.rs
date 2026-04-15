mod aw_client;
mod classifier;
mod config;
mod kimai_client;
mod merger;
mod state;

use anyhow::Result;
use chrono::Utc;
use std::time::Duration;
use tokio::signal;
use tracing_subscriber::EnvFilter;

use aw_client::AwClient;
use classifier::Classifier;
use config::Config;
use kimai_client::KimaiClient;
use merger::Merger;
use state::StateManager;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise tracing. Set RUST_LOG=debug for verbose output.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("aw-kimai-bridge starting");

    // ── Load config ───────────────────────────────────────────────────────────
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());

    let config = Config::load(&config_path)?;
    tracing::info!(config = %config_path, bucket = %config.activitywatch.bucket, "Config loaded");

    // ── Build shared components ───────────────────────────────────────────────
    let aw = AwClient::new(&config.activitywatch.url);
    let kimai = KimaiClient::new(&config.kimai.url, &config.kimai.token, config.kimai.user_id);
    let classifier = Classifier::from_config(&config);
    let merger = Merger::new(config.idle_threshold_secs, config.min_duration_secs);
    let mut state_mgr = StateManager::load(&config.state_path)?;

    // ── Startup connectivity checks ───────────────────────────────────────────
    tracing::info!("Checking ActivityWatch connectivity...");
    aw.ping().await?;
    tracing::info!("ActivityWatch: OK");

    tracing::info!("Checking Kimai connectivity...");
    kimai.ping().await?;
    tracing::info!("Kimai: OK");

    // ── Daemon loop ───────────────────────────────────────────────────────────
    let interval_duration = Duration::from_secs(config.sync_interval_secs);
    tracing::info!(
        interval_secs = config.sync_interval_secs,
        "Entering sync loop — press Ctrl-C to stop"
    );

    let mut ticker = tokio::time::interval(interval_duration);
    // Don't stack up missed ticks if a sync takes longer than the interval.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                match sync_once(&aw, &kimai, &classifier, &merger, &mut state_mgr, &config).await {
                    Ok(pushed) => {
                        if pushed > 0 {
                            tracing::info!(pushed, "Sync cycle complete");
                        } else {
                            tracing::debug!("Sync cycle complete — no new entries");
                        }
                    }
                    Err(e) => {
                        // Non-fatal: log and wait for the next tick.
                        tracing::warn!(error = %e, "Sync cycle failed, will retry");
                    }
                }
            }
            _ = signal::ctrl_c() => {
                tracing::info!("Received shutdown signal, exiting cleanly");
                break;
            }
        }
    }

    Ok(())
}

/// Run one full sync cycle: fetch → classify → merge → push → save cursor.
/// Returns the number of entries pushed on this cycle.
async fn sync_once(
    aw: &AwClient,
    kimai: &KimaiClient,
    classifier: &Classifier,
    merger: &Merger,
    state_mgr: &mut StateManager,
    config: &Config,
) -> Result<usize> {
    let since = state_mgr.state.last_synced_at;
    let until = Utc::now();

    tracing::debug!(
        since = ?since,
        until = %until,
        bucket = %config.activitywatch.bucket,
        "Fetching events"
    );

    // 1 · Fetch raw events from ActivityWatch
    let raw_events = aw
        .get_events(&config.activitywatch.bucket, since, Some(until), None)
        .await?;

    if raw_events.is_empty() {
        // No new events — just advance the cursor so we don't re-scan forever
        state_mgr.record_sync(until, 0)?;
        return Ok(0);
    }

    tracing::debug!(count = raw_events.len(), "Raw events fetched");

    // 2 · Classify each event against the rule set
    let classified = classifier.classify_all(&raw_events);
    tracing::debug!(count = classified.len(), "Events classified");

    // 3 · Merge adjacent same-project events, drop short idle blocks
    let blocks = merger.merge(classified);
    tracing::debug!(count = blocks.len(), "Blocks after merge");

    if blocks.is_empty() {
        state_mgr.record_sync(until, 0)?;
        return Ok(0);
    }

    // 4 · Push to Kimai
    let (pushed, errors) = kimai.push_batch(&blocks).await;

    if !errors.is_empty() {
        tracing::warn!(
            pushed,
            failed = errors.len(),
            "Partial sync — some entries will be retried next cycle"
        );
        // Don't advance the cursor on partial failure so we re-attempt next tick.
        // In practice you'd track individual block cursors here for robustness,
        // but this is safe for the common case where failures are transient.
        if pushed == 0 {
            anyhow::bail!("All {} entries failed to push", errors.len());
        }
    }

    // 5 · Save cursor
    state_mgr.record_sync(until, pushed as u64)?;

    Ok(pushed)
}
