# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A Rust daemon that syncs ActivityWatch window-tracking events into Kimai timesheet entries. Pulls window events from a local ActivityWatch instance, subtracts AFK periods, classifies events by regex rules, merges adjacent events into contiguous time blocks, and pushes them to Kimai via API.

Two binaries are built:
- **aw-kimai-bridge**: the sync daemon. Also installs/uninstalls itself as a per-user OS service.
- **aw-kimai-admin**: a CLI for managing Kimai customers/projects/activities and for the first-run setup wizard.

## Common Commands

```bash
# Build / test / lint
cargo build
cargo build --release
cargo test
cargo test merger::tests::merges_adjacent_same_project   # run a single test
cargo clippy --all-targets                                # required before PRs

# Daemon — note the clap subcommands
cargo run --bin aw-kimai-bridge -- run config.toml
cargo run --bin aw-kimai-bridge -- install-service config.toml
cargo run --bin aw-kimai-bridge -- uninstall-service
cargo run --bin aw-kimai-bridge -- service-status

# Admin CLI
cargo run --bin aw-kimai-admin -- setup                   # interactive wizard
cargo run --bin aw-kimai-admin -- bootstrap init-blueprint acme.toml
cargo run --bin aw-kimai-admin -- --help

# Debug logging
RUST_LOG=debug cargo run --bin aw-kimai-bridge -- run config.toml
```

`aw-kimai-bridge` with no args defaults to `run config.toml`. The bare positional path form (`aw-kimai-bridge config.toml`) was removed when subcommands were added — call sites and docs must use `run` explicitly.

## Architecture

### Data flow (daemon, one cycle)

```
ActivityWatch ──► aw_client ──► afk::clip_events ──► classifier ──► merger ──► kimai_client ──► Kimai
                  (window +     (subtract AFK         (regex rules,    (merge blocks,     (timesheet
                   AFK buckets)  intervals from        first match     drop short        entries)
                                 window events)        wins)            blocks)
```

Steps per `sync_once` in [src/main.rs](src/main.rs):
1. **Fetch window events** from `[activitywatch].bucket` since `state.last_synced_at`.
2. **Fetch AFK events** from `[activitywatch].afk_bucket` (when configured) with a 24h lookback so a long AFK window that began before the cursor still clips correctly.
3. **Clip** ([src/afk.rs](src/afk.rs)) — `intervals_from_events` merges AFK intervals; `clip_events` subtracts them from each window event, splitting events that straddle AFK and dropping events fully covered by AFK. Fragments shorter than 1s are discarded as noise. When `afk_bucket` is unset the daemon warns at startup and skips this step.
4. **Classify** ([src/classifier.rs](src/classifier.rs)) — first match wins against a haystack of `title\napp\nurl`. `project_id = 0` is the explicit "ignore this event" sentinel.
5. **Merge** ([src/merger.rs](src/merger.rs)) — same-project events with gaps ≤ `idle_threshold_secs` collapse into one `TimeBlock`. Blocks shorter than `min_duration_secs` are dropped.
6. **Push** ([src/kimai_client.rs](src/kimai_client.rs)) — `push_batch` returns `Vec<Result<TimesheetEntry>>` (one per block, in order).
7. **Advance cursor** — to `until` on full success, or to the first failed block's `start` on partial failure so the failed block (and anything after) gets retried. Previously failures were silently dropped; do not regress.

### Module structure

**Daemon (`src/`)**:
- `main.rs` — clap CLI (`run` / `install-service` / `uninstall-service` / `service-status`); `sync_once` orchestrates the cycle.
- `config.rs` — TOML config. **`KIMAI_TOKEN` env var always overrides `kimai.token` in the file.** Both binaries enforce this; the admin CLI's `config.rs` mirrors the precedence.
- `aw_client.rs` — HTTP client for AW. Separate `get_events` (window) and `get_afk_events` paths because the `data` payloads have different shapes.
- `afk.rs` — pure functions: `intervals_from_events` (sorted, merged) and `clip_events` (splits/drops window events against AFK intervals). Heavily unit-tested; new AFK-related logic belongs here, not in main or the merger.
- `classifier.rs` — compiles regexes at startup (`Config::validate` pre-checks them so we fail fast).
- `merger.rs` — `BlockBuilder` accumulates `(title, app, url) → seconds` per block; `build_description` sorts by contributed time, takes the top `MAX_DETAILS_SHOWN = 5`, formats `"title [app] <url> | … (+N more)"`, and `truncate(_, 255)` happens at push time. The first window title is no longer the description — preserve this multi-event aggregation when changing the merger.
- `kimai_client.rs` — POST `/api/timesheets`. **Timestamps must include a tz offset (`%Y-%m-%dT%H:%M:%S%:z`)** — without it Kimai interprets them as server-local and silently shifts entries by the local offset. Same applies to the `begin=` query on `list_recent_entries`.
- `service.rs` — per-user auto-start installer. macOS = launchd plist at `~/Library/LaunchAgents`, Linux = systemd `--user` unit at `~/.config/systemd/user`, Windows = `.cmd` shim in the Startup folder. Each `platform` submodule is `#[cfg(target_os = …)]`-gated; a no-op fallback covers other OSes so the build never fails. No admin rights required.
- `state.rs` — sync cursor persisted via write-then-rename. Delete the file to force a full re-sync.

**Admin CLI (`src/bin/admin/`)**:
- `main.rs` — clap subcommands for customer/project/activity CRUD + `bootstrap` + `setup`. `setup` is special-cased before `Config::load` because it *creates* the config.
- `setup.rs` — 4-step wizard. Calls `GET /api/0/buckets/` to auto-discover window + AFK buckets (uses `reqwest::blocking`, hence the `blocking` feature on reqwest); falls back to hostname-based guesses when AW isn't reachable. Lets the user leave the token blank to defer to `KIMAI_TOKEN`. Ends by printing the `install-service` command.
- `kimai_admin/mod.rs` — `KimaiAdminClient` (separate from the daemon's `KimaiClient` because the admin surface is much wider).
- `kimai_admin/bootstrap.rs` — idempotent customer/project/activity scaffolding from a TOML blueprint or interactive prompts.

### Key types

- `AwEvent` / `AwEventData` — raw window event with `title`, `app`, `url`.
- `AfkEvent` / `AfkEventData` — raw AFK event with `status` (`"afk"` / `"not-afk"`). `is_afk()` is the canonical predicate.
- `ClassifiedEvent` — `AwEvent` + assigned `project_id` / `activity_id` / `label`.
- `TimeBlock` — merged block with aggregated description.
- `State` — `last_synced_at`, `total_entries_pushed`, `daemon_started_at`.

### Configuration

```toml
[kimai]
url                = "https://kimai.example.com"
# token is read from KIMAI_TOKEN env var if set; otherwise from here
token              = "ki_..."
default_project_id = 1
default_activity_id = 1

[activitywatch]
url        = "http://localhost:5600"
bucket     = "aw-watcher-window_HOSTNAME"
afk_bucket = "aw-watcher-afk_HOSTNAME"   # optional but strongly recommended

sync_interval_secs  = 300
idle_threshold_secs = 120
min_duration_secs   = 60

[[rules]]
pattern    = "github\.com/myorg"   # case-insensitive regex against title\napp\nurl
project_id = 10
activity_id = 2
label      = "MyOrg Work"
```

## Testing

Unit tests live in source files under `#[cfg(test)]`. Coverage worth knowing about:
- `afk.rs` — clip/split/drop and interval merging.
- `merger.rs` — adjacency merging, splits, short-block drops, **and description-shape assertions** (don't change description formatting without updating those).
- `classifier.rs` — rule matching, URL matching, ignore rules, fall-through.

`aw_client.rs` and `kimai_client.rs` are HTTP boundaries with no unit tests.

## CI/CD

`.github/workflows/release.yml` builds Linux/macOS/Windows x86-64 and ARM64. Linux ARM64 uses `cross`; everything else builds natively. Tests run on native targets only. Tags matching `v*` produce release archives.

## Conventions

- **TLS**: `reqwest` is configured `default-features = false, features = ["json", "rustls-tls", "blocking"]` so we have no OpenSSL dependency and can build fully static musl binaries. Don't reintroduce `native-tls`.
- **Time**: everything internal is `chrono::DateTime<Utc>`. Only the Kimai POST formatter touches timezone strings — and it must keep the `%:z` suffix.
- **Logging**: `tracing` with `RUST_LOG`. `info` = sync summaries, `debug` = per-event detail, `trace` = individual rule matches.
- **State**: never advance the cursor past a known failure. The current logic in `main.rs` is load-bearing — if you refactor `push_batch`, keep per-block result ordering so the "first failure timestamp" calculation still works.
