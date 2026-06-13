mod afk;
mod aw_client;
mod classifier;
mod config;
mod kimai_client;
mod merger;
mod service;
mod state;

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use std::time::Duration;
use tokio::signal;
use tracing_subscriber::EnvFilter;

use aw_client::AwClient;
use classifier::Classifier;
use config::Config;
use kimai_client::KimaiClient;
use merger::Merger;
use state::StateManager;

/// aw-kimai-bridge — daemon that syncs ActivityWatch window events to Kimai.
#[derive(Parser)]
#[command(name = "aw-kimai-bridge")]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the sync daemon (the default when no subcommand is given).
    Run {
        /// Path to config file
        #[arg(default_value = "config.toml")]
        config: String,
    },
    /// Install as a per-user auto-start service. After this you don't need
    /// to keep a terminal open — the bridge runs in the background and
    /// restarts on login.
    InstallService {
        /// Path to config file the service should use. Stored as an absolute
        /// path so the service can find it regardless of working directory.
        #[arg(default_value = "config.toml")]
        config: String,
    },
    /// Remove the auto-start service installed by `install-service`.
    UninstallService,
    /// Print whether the service is currently installed and where.
    ServiceStatus,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Run {
        config: "config.toml".to_string(),
    });

    match command {
        Command::Run { config } => run_daemon(&config).await,
        Command::InstallService { config } => service::install(&config),
        Command::UninstallService => service::uninstall(),
        Command::ServiceStatus => service::status(),
    }
}

async fn run_daemon(config_path: &str) -> Result<()> {
    tracing::info!("aw-kimai-bridge starting");

    let config = Config::load(config_path)?;
    tracing::info!(
        config = %config_path,
        bucket = %config.activitywatch.bucket,
        afk_bucket = ?config.activitywatch.afk_bucket,
        "Config loaded"
    );

    let aw = AwClient::new(&config.activitywatch.url);
    let kimai = KimaiClient::new(&config.kimai.url, &config.kimai.token, config.kimai.user_id);
    let classifier = Classifier::from_config(&config);
    let merger = Merger::new(config.idle_threshold_secs, config.min_duration_secs);
    let mut state_mgr = StateManager::load(&config.state_path)?;

    tracing::info!("Checking ActivityWatch connectivity...");
    aw.ping().await?;
    tracing::info!("ActivityWatch: OK");

    tracing::info!("Checking Kimai connectivity...");
    kimai.ping().await?;
    tracing::info!("Kimai: OK");

    if config.activitywatch.afk_bucket.is_none() {
        tracing::warn!(
            "No [activitywatch].afk_bucket configured — AFK time will be billed. \
             Add `afk_bucket = \"aw-watcher-afk_<hostname>\"` to fix."
        );
    }

    let interval_duration = Duration::from_secs(config.sync_interval_secs);
    tracing::info!(
        interval_secs = config.sync_interval_secs,
        "Entering sync loop — press Ctrl-C to stop"
    );

    let mut ticker = tokio::time::interval(interval_duration);
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

/// Run one full sync cycle: fetch → AFK-clip → classify → merge → push → save cursor.
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

    // 1 · Fetch raw window events from ActivityWatch
    let raw_events = aw
        .get_events(&config.activitywatch.bucket, since, Some(until), None)
        .await?;

    if raw_events.is_empty() {
        state_mgr.record_sync(until, 0)?;
        return Ok(0);
    }

    tracing::debug!(count = raw_events.len(), "Raw events fetched");

    // 2 · If configured, fetch AFK events and subtract them so we never bill
    //     time the user was away from the keyboard.
    let active_events = if let Some(afk_bucket) = &config.activitywatch.afk_bucket {
        // Pull AFK events with some lookback so a long AFK window that started
        // before `since` still clips correctly.
        let afk_since = since.map(|s| s - chrono::Duration::hours(24));
        let afk_events = aw
            .get_afk_events(afk_bucket, afk_since, Some(until))
            .await?;
        let intervals = afk::intervals_from_events(&afk_events);
        tracing::debug!(
            afk_events = afk_events.len(),
            afk_intervals = intervals.len(),
            "Fetched AFK events"
        );
        afk::clip_events(raw_events, &intervals)
    } else {
        raw_events
    };

    if active_events.is_empty() {
        state_mgr.record_sync(until, 0)?;
        return Ok(0);
    }

    // 3 · Classify each event against the rule set
    let classified = classifier.classify_all(&active_events);
    tracing::debug!(count = classified.len(), "Events classified");

    // 4 · Merge adjacent same-project events, drop short idle blocks
    let blocks = merger.merge(classified);
    tracing::debug!(count = blocks.len(), "Blocks after merge");

    if blocks.is_empty() {
        state_mgr.record_sync(until, 0)?;
        return Ok(0);
    }

    // 5 · Push to Kimai. We get one Result per block so we can find the
    //     first failure and rewind the cursor just enough to retry it.
    let results = kimai.push_batch(&blocks).await;

    let mut pushed = 0usize;
    let mut first_failure_start: Option<DateTime<Utc>> = None;
    for (i, r) in results.iter().enumerate() {
        match r {
            Ok(_) => pushed += 1,
            Err(_) => {
                if first_failure_start.is_none() {
                    first_failure_start = Some(blocks[i].start);
                }
            }
        }
    }

    let failures = results.len() - pushed;
    if failures > 0 {
        tracing::warn!(
            pushed,
            failed = failures,
            "Partial sync — failed entries will be retried next cycle"
        );
    }

    // 6 · Save cursor.
    //
    // If everything succeeded, advance to `until`. If any block failed,
    // park the cursor at the failed block's start so the next cycle re-fetches
    // and retries it. Successful blocks after the failure may be retried too;
    // the existing min_duration_secs floor + Kimai server-side dedup are the
    // user's safety nets there. The previous behavior silently dropped failed
    // blocks, which was worse.
    let cursor = first_failure_start.unwrap_or(until);
    state_mgr.record_sync(cursor, pushed as u64)?;

    Ok(pushed)
}
