# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A Rust daemon that syncs ActivityWatch window-tracking events into Kimai timesheet entries. It classifies window events by regex rules, merges adjacent events into contiguous time blocks, and pushes them to Kimai via API.

Two binaries are built:
- **aw-kimai-bridge**: The main daemon that runs continuously, syncing on a schedule
- **aw-kimai-admin**: A CLI tool for managing Kimai customers, projects, and activities

## Common Commands

```bash
# Build debug binaries
cargo build

# Build release binaries
cargo build --release

# Run tests
cargo test

# Run linting (required before PRs)
cargo clippy

# Run the daemon (requires config.toml)
cargo run --bin aw-kimai-bridge -- config.toml

# Run the admin CLI
cargo run --bin aw-kimai-admin -- --help

# Generate a bootstrap blueprint template
cargo run --bin aw-kimai-admin -- bootstrap init-blueprint acme.toml

# Run with debug logging
RUST_LOG=debug cargo run --bin aw-kimai-bridge -- config.toml
```

## Architecture

### Data Flow (Daemon)

```
ActivityWatch ──► aw_client ──► classifier ──► merger ──► kimai_client ──► Kimai
                                 (regex)       (merge      (timesheet
                                  rules        blocks)      entries)
```

Each sync cycle:
1. **aw_client.rs**: Fetches raw window events from ActivityWatch HTTP API (`/api/0/buckets/{id}/events`)
2. **classifier.rs**: Matches events against ordered regex rules (first match wins). `project_id = 0` means ignore
3. **merger.rs**: Merges adjacent same-project events if gap < `idle_threshold_secs`, drops blocks shorter than `min_duration_secs`
4. **kimai_client.rs**: POSTs merged TimeBlocks to Kimai `/api/timesheets`
5. **state.rs**: Atomically writes sync cursor to `state.json` (last_synced_at)

### Module Structure

**Daemon (src/main.rs)**:
- `config.rs`: TOML config with `[kimai]`, `[activitywatch]`, `[[rules]]` sections
- `aw_client.rs`: HTTP client for ActivityWatch API; fetches bucket events
- `kimai_client.rs`: HTTP client for Kimai API; pushes timesheet entries
- `classifier.rs`: Compiles regex rules, classifies `AwEvent` → `ClassifiedEvent`
- `merger.rs`: Merges `ClassifiedEvent` list → `Vec<TimeBlock>`
- `state.rs`: JSON persistence for sync cursor with atomic write-then-rename

**Admin CLI (src/bin/admin/)**:
- `main.rs`: clap CLI with customer/project/activity/bootstrap subcommands
- `kimai_admin/mod.rs`: `KimaiAdminClient` for Kimai entity management APIs
- `kimai_admin/bootstrap.rs`: Interactive wizard + idempotent blueprint application
- `config.rs`: Subset of daemon config (only `[kimai]` section needed)
- `output.rs`: Table and JSON formatting for CLI output
- `setup.rs`: Interactive first-run configuration wizard

### Key Types

- `AwEvent`: Raw ActivityWatch event with timestamp, duration, title, app, optional URL
- `ClassifiedEvent`: AwEvent + project_id/activity_id/label from rule matching
- `TimeBlock`: Merged contiguous work block with start/end/project/activity/description
- `State`: Persisted cursor with last_synced_at, total_entries_pushed, daemon_started_at

### Configuration

Rules are evaluated top-to-bottom; first match wins. Patterns match against "title\napp\nurl" combined string (case-insensitive).

```toml
[kimai]
url = "https://kimai.example.com"
token = "ki_..."
default_project_id = 1
default_activity_id = 1

[activitywatch]
bucket = "aw-watcher-window_hostname"

sync_interval_secs = 300
idle_threshold_secs = 120  # merge gaps under 2 min
min_duration_secs = 60     # drop blocks under 1 min

[[rules]]
pattern = "github\.com/myorg"
project_id = 10
activity_id = 2
label = "MyOrg Work"

[[rules]]
pattern = "YouTube|Reddit"
project_id = 0  # ignore
```

## Testing

Unit tests are embedded in source files under `#[cfg(test)]`. Key test areas:
- `classifier.rs`: Tests rule matching, URL matching, ignore rules, fall-through to default
- `merger.rs`: Tests merging adjacent events, splitting on project changes, dropping short blocks

## CI/CD

GitHub Actions workflow (`.github/workflows/release.yml`):
- Builds for Linux x86-64/ARM64, macOS x86-64/ARM64, Windows x86-64/ARM64
- Cross-compilation uses `cross` for Linux ARM64
- Tests run on native targets
- Creates release archives on version tags (`v*`)

## State Management

The daemon writes `state.json` atomically (write to `.json.tmp`, then rename) after each successful sync cycle. The `last_synced_at` timestamp is the cursor for the next ActivityWatch query. To force a full re-sync, delete `state.json`.

## Logging

Uses `tracing` with `RUST_LOG` environment variable:
- `RUST_LOG=info`: Sync summaries only
- `RUST_LOG=debug`: Per-event classification and merge details
- `RUST_LOG=trace`: Individual rule match logging
