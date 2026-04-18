# aw-kimai-bridge

A daemon that automatically syncs [ActivityWatch](https://activitywatch.net/) window-tracking events into [Kimai](https://www.kimai.org/) as timesheet entries. It runs in the background, wakes up on a configurable interval, classifies your active windows against a set of regex rules, merges adjacent events into clean time blocks, and pushes them to Kimai — with no manual input required.

Also ships `aw-kimai-admin`, a companion CLI for managing Kimai customers, projects, and activities without leaving the terminal.

---

## Table of Contents

- [How it works](#how-it-works)
- [Requirements](#requirements)
- [Installation](#installation)
- [Setup](./aw_kimai_bridge_setup_guide_styled.md)
- [Quick start](#quick-start)
- [Configuration](#configuration)
  - [Kimai](#kimai)
  - [ActivityWatch](#activitywatch)
  - [Daemon tuning](#daemon-tuning)
  - [Classification rules](#classification-rules)
  - [Full example](#full-example)
- [aw-kimai-admin CLI](#aw-kimai-admin-cli)
  - [Customers](#customers)
  - [Projects](#projects)
  - [Activities](#activities)
  - [Bootstrap](#bootstrap)
- [State file](#state-file)
- [Logging](#logging)
- [Building from source](#building-from-source)
- [Contributing](#contributing)
- [License](#license)

---

## How it works

```
ActivityWatch            aw-kimai-bridge              Kimai
─────────────            ───────────────              ─────
Window events   ──────►  1. Fetch events
                         2. Classify by rule   ──────► project / activity IDs
                         3. Merge blocks              (gaps < threshold bridged)
                         4. Drop short blocks         (< min duration)
                         5. Push timesheets   ──────► POST /api/timesheets
                         6. Save cursor               (state.json)
```

Each sync cycle:

1. **Fetch** — queries ActivityWatch for all events since the last successful sync.
2. **Classify** — each event's window title, app name, and URL (if a browser watcher is running) are tested against your ordered regex rules. First match wins. Events matching a rule with `project_id = 0` are silently discarded (useful for ignoring distractions).
3. **Merge** — adjacent events belonging to the same project/activity are merged into a single `TimeBlock` if the gap between them is shorter than `idle_threshold_secs`. This handles brief alt-tabs and context switches cleanly.
4. **Filter** — blocks shorter than `min_duration_secs` are dropped.
5. **Push** — each surviving block is posted to Kimai's timesheet API.
6. **Persist** — the sync cursor (end timestamp of this cycle) is written atomically to `state.json`. If a cycle partially fails, the cursor is not advanced so affected entries are retried next tick.

---

## Requirements

- [ActivityWatch](https://activitywatch.net/) running locally with at least `aw-watcher-window` active.
- A Kimai instance (self-hosted or cloud) with API access and a valid API token.
- Rust 1.75+ if building from source. Pre-built binaries are available on the [Releases](../../releases) page.

---

## Installation

### Pre-built binaries (recommended)

Download the archive for your platform from the [Releases](../../releases) page, extract it, and place the binaries somewhere on your `$PATH`.

| Platform       | Archive                                  |
|----------------|------------------------------------------|
| Linux x86-64   | `aw-kimai-bridge-linux-amd64.tar.gz`     |
| Linux ARM64    | `aw-kimai-bridge-linux-arm64.tar.gz`     |
| macOS x86-64   | `aw-kimai-bridge-macos-amd64.tar.gz`     |
| macOS ARM64    | `aw-kimai-bridge-macos-arm64.tar.gz`     |
| Windows x86-64 | `aw-kimai-bridge-windows-amd64.zip`      |
| Windows ARM64  | `aw-kimai-bridge-windows-arm64.zip`      |

Verify the download against `SHA256SUMS.txt` before running.

### From source

```bash
git clone https://github.com/yourorg/aw-kimai-bridge
cd aw-kimai-bridge
cargo build --release
# binaries land at target/release/aw-kimai-bridge and target/release/aw-kimai-admin
```

---

## Quick start

1. **Find your ActivityWatch bucket name.** Open the ActivityWatch web UI at `http://localhost:5600`, go to *Buckets*, and copy the name of your window-watcher bucket — it typically looks like `aw-watcher-window_yourhostname`.

2. **Get your Kimai API token.** In Kimai go to your profile → *API access* and generate a token. Note the project and activity IDs you want to use as defaults (visible in the Kimai admin or via `aw-kimai-admin project list`).

3. **Create `config.toml`:**

```toml
[kimai]
url                = "https://kimai.example.com"
token              = "your-api-token"
default_project_id = 1
default_activity_id = 1

[activitywatch]
bucket = "aw-watcher-window_yourhostname"
```

4. **Run the daemon:**

```bash
aw-kimai-bridge config.toml
```

The daemon will log to stdout and sync every 5 minutes by default. Press `Ctrl-C` for a clean shutdown.

---

## Configuration

All configuration lives in a single TOML file. Pass its path as the first argument (`config.toml` is the default).

### Kimai

```toml
[kimai]
url                 = "https://kimai.example.com"   # Required
token               = "your-api-token"              # Required
default_project_id  = 1                             # Required — fallback when no rule matches
default_activity_id = 1                             # Required — fallback when no rule matches
user_id             = 3                             # Optional — assign entries to a specific user
```

`user_id` is optional. If omitted, Kimai assigns entries to the user who owns the token.

### ActivityWatch

```toml
[activitywatch]
url    = "http://localhost:5600"              # Optional — default shown
bucket = "aw-watcher-window_yourhostname"    # Required
```

### Daemon tuning

```toml
sync_interval_secs  = 300   # How often to sync (default: 300 = 5 minutes)
idle_threshold_secs = 120   # Max gap (seconds) to bridge when merging same-project events (default: 120)
min_duration_secs   = 60    # Minimum block length to push to Kimai (default: 60)
state_path          = "state.json"  # Where to persist the sync cursor (default: state.json)
```

**`idle_threshold_secs`** controls how forgiving the merger is about context switches. A value of 120 means that if you switch away from your project window for less than 2 minutes and come back, it counts as uninterrupted work.

**`min_duration_secs`** prevents noise: accidental window focuses, brief glances at other apps, and other sub-minute events never make it into Kimai.

### Classification rules

Rules are evaluated top-to-bottom; the first match wins. Each rule is tested against a combined string of the event's window title, app name, and URL (if a browser watcher is active), separated by newlines — so a single pattern can match any of the three fields.

```toml
[[rules]]
pattern    = "github\\.com/myorg"   # Case-insensitive regex
project_id = 10
activity_id = 2
label      = "myorg — GitHub"       # Optional; shown in logs instead of the raw pattern

[[rules]]
pattern    = "JIRA-\\d+"
project_id = 10
activity_id = 3
label      = "myorg — Jira"

# Ignore distractions — project_id = 0 drops matching events entirely
[[rules]]
pattern    = "YouTube|Twitter|Reddit|Hacker News"
project_id  = 0
activity_id = 0
label       = "ignore"
```

Events that match no rule fall through to `default_project_id` / `default_activity_id`.

### Full example

```toml
[kimai]
url                 = "https://kimai.example.com"
token               = "ki_abc123"
default_project_id  = 1
default_activity_id = 1
user_id             = 3

[activitywatch]
url    = "http://localhost:5600"
bucket = "aw-watcher-window_mylaptop"

sync_interval_secs  = 300
idle_threshold_secs = 120
min_duration_secs   = 60
state_path          = "/var/lib/aw-kimai/state.json"

# ── Rules (first match wins) ──────────────────────────────────────────────

[[rules]]
pattern     = "github\\.com/acme"
project_id  = 10
activity_id = 2
label       = "Acme — GitHub"

[[rules]]
pattern     = "ACME-\\d+"
project_id  = 10
activity_id = 3
label       = "Acme — Jira"

[[rules]]
pattern     = "figma\\.com"
project_id  = 11
activity_id = 4
label       = "Acme — Design"

[[rules]]
pattern     = "Slack|Teams|Zoom|Meet"
project_id  = 12
activity_id = 5
label       = "Internal comms"

[[rules]]
pattern     = "YouTube|Netflix|Twitch|Reddit"
project_id  = 0
activity_id = 0
label       = "ignore"
```

---

## aw-kimai-admin CLI

`aw-kimai-admin` is a companion tool for managing the Kimai entity hierarchy (customers → projects → activities) from the terminal. It reads the same `config.toml` (only the `[kimai]` section is required).

```
Usage: aw-kimai-admin [OPTIONS] <COMMAND>

Options:
  -c, --config <FILE>        Path to config file [default: config.toml]
  -o, --output <FORMAT>      Output format: table (default) or json

Commands:
  customer    Manage customers
  project     Manage projects
  activity    Manage activities
  bootstrap   Scaffold a full team setup in one shot
```

### Customers

```bash
aw-kimai-admin customer list
aw-kimai-admin customer get <ID>
aw-kimai-admin customer create "Acme Corp" --email billing@acme.com --currency USD --timezone America/Chicago --budget 500
aw-kimai-admin customer delete <ID> [--force]
```

### Projects

```bash
aw-kimai-admin project list [--customer <ID>]
aw-kimai-admin project get <ID>
aw-kimai-admin project create "Website Redesign" --customer <ID> --color "#4CAF50" --budget 200 --start 2026-01-01 --end 2026-06-30
aw-kimai-admin project delete <ID> [--force]
```

### Activities

```bash
aw-kimai-admin activity list [--project <ID>]
aw-kimai-admin activity get <ID>
aw-kimai-admin activity create "Frontend development" --project <ID> --color "#2196F3"
aw-kimai-admin activity delete <ID> [--force]
```

### Bootstrap

The bootstrap command creates a full customer → project → activity hierarchy in one shot. It is idempotent: if a customer, project, or activity with the same name already exists it is reused rather than duplicated.

**Generate a blueprint template:**

```bash
aw-kimai-admin bootstrap init-blueprint acme.toml
```

Edit the generated `acme.toml`:

```toml
[[customers]]
name     = "Acme Corp"
currency = "USD"
timezone = "America/Chicago"
email    = "billing@acme.example"
budget   = 500.0

  [[customers.projects]]
  name   = "Website Redesign"
  color  = "#4CAF50"
  budget = 200.0
  start  = "2026-01-01"
  end    = "2026-06-30"

    [[customers.projects.activities]]
    name  = "Frontend development"
    color = "#2196F3"

    [[customers.projects.activities]]
    name  = "Backend development"
    color = "#FF9800"

    [[customers.projects.activities]]
    name  = "Code review"
```

**Apply the blueprint:**

```bash
aw-kimai-admin bootstrap run --from-file acme.toml
```

The command prints each created entity with its Kimai ID so you can paste the IDs directly into your `config.toml` rules.

**Interactive wizard** (no file needed):

```bash
aw-kimai-admin bootstrap run
```

---

## State file

The daemon writes a small JSON file (default: `state.json`) after every successful sync cycle:

```json
{
  "last_synced_at": "2026-04-15T14:00:00Z",
  "total_entries_pushed": 142,
  "daemon_started_at": "2026-04-01T08:00:00Z"
}
```

The file is written atomically (write to `.json.tmp`, then rename) so it cannot be corrupted by an unclean shutdown. On the next startup the daemon reads `last_synced_at` and only fetches events from that point forward. To force a full re-sync from the beginning, delete `state.json` before starting the daemon.

---

## Logging

Log verbosity is controlled by the `RUST_LOG` environment variable (powered by [`tracing-subscriber`](https://docs.rs/tracing-subscriber)):

```bash
RUST_LOG=info  aw-kimai-bridge config.toml   # default — sync summaries only
RUST_LOG=debug aw-kimai-bridge config.toml   # per-event classification and merge details
RUST_LOG=trace aw-kimai-bridge config.toml   # everything, including individual rule matches
```

---

## Building from source

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Run the daemon
cargo run --bin aw-kimai-bridge -- config.toml

# Run the admin CLI
cargo run --bin aw-kimai-admin -- --help
```

Cross-compilation targets and CI configuration are defined in `.github/workflows/release.yml`. See that file for the full matrix of supported platforms.

---

## Contributing

Bug reports and pull requests are welcome. Please open an issue before starting significant work so we can discuss the approach first.

Run `cargo test` and `cargo clippy` before submitting a PR.

---

## License

MIT — see [LICENSE](LICENSE).