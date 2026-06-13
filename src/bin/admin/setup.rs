// src/bin/admin/setup.rs
//
// Interactive `aw-kimai-admin setup` command.
// Walks the user through the required values, auto-discovers ActivityWatch
// buckets where possible, and writes a ready-to-use config.toml.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Write};

// ── Public entry point ────────────────────────────────────────────────────

pub fn run(output: &str, dry_run: bool) -> Result<()> {
    print_banner();

    // Step 1 — Kimai URL
    let kimai_url = step_kimai_url()?;

    // Step 2 — Kimai API token (or env-var pointer)
    let (kimai_token, token_in_env) = step_kimai_token()?;

    // Step 3 — ActivityWatch URL
    let aw_url = step_aw_url()?;

    // Step 4 — pick window + AFK buckets (auto-discover if possible)
    let (window_bucket, afk_bucket) = step_buckets(&aw_url)?;

    let toml = render_config(
        &kimai_url,
        &kimai_token,
        token_in_env,
        &aw_url,
        &window_bucket,
        afk_bucket.as_deref(),
    );

    if dry_run {
        println!("\n{}\n", "─".repeat(60));
        println!("# Generated config.toml (dry-run — nothing written)\n");
        println!("{toml}");
        println!("{}", "─".repeat(60));
        return Ok(());
    }

    if std::path::Path::new(output).exists() {
        print!("\n⚠  '{}' already exists. Overwrite? [y/N] ", output);
        io::stdout().flush()?;
        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;
        if buf.trim().to_lowercase() != "y" {
            println!("Aborted — existing config left untouched.");
            return Ok(());
        }
    }

    std::fs::write(output, &toml)
        .with_context(|| format!("Failed to write config to '{output}'"))?;

    println!("\n✓  Config written to '{output}'");
    println!("   Next steps:");
    println!("     1. Edit [[rules]] in '{output}' to map windows → projects.");
    println!("        (Use `aw-kimai-admin project list` to find IDs.)");
    println!(
        "     2. Install as a background service so the bridge runs automatically:"
    );
    println!("            aw-kimai-bridge install-service {output}");
    println!("        Or run it manually for testing:");
    println!("            aw-kimai-bridge run {output}");

    Ok(())
}

// ── Step helpers ──────────────────────────────────────────────────────────

fn step_kimai_url() -> Result<String> {
    println!("\nStep 1 of 4 — Kimai URL");
    println!("  The base URL of your Kimai instance, e.g. https://kimai.example.com");

    loop {
        let raw = prompt("  Kimai URL")?;
        let url = raw.trim_end_matches('/').to_string();

        if url.is_empty() {
            eprintln!("  ✗  URL cannot be empty, please try again.");
            continue;
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            eprintln!("  ✗  URL must start with http:// or https://");
            continue;
        }

        println!("  ✓  Kimai URL: {url}");
        return Ok(url);
    }
}

/// Returns `(token_to_store_in_toml, token_came_from_env)`. When the token
/// comes from `KIMAI_TOKEN`, we leave the file blank and add a comment so the
/// secret never lands in plaintext.
fn step_kimai_token() -> Result<(String, bool)> {
    println!("\nStep 2 of 4 — Kimai API token");
    println!("  Where to find it:");
    println!("    Kimai → top-right avatar → My profile → API → Create token");
    println!("  The token is shown only once — copy it now.");
    println!();
    println!("  If you'd rather not store it in config.toml, set the");
    println!("  KIMAI_TOKEN environment variable instead and press Enter here.");

    loop {
        let token = read_secret("  API token (or blank to use $KIMAI_TOKEN)")?;

        if token.is_empty() {
            if std::env::var("KIMAI_TOKEN").map(|v| !v.is_empty()).unwrap_or(false) {
                println!("  ✓  Will use the KIMAI_TOKEN environment variable.");
                return Ok((String::new(), true));
            }
            eprintln!(
                "  ✗  Token blank and KIMAI_TOKEN env var not set. \
                 Set the env var first, or paste the token here."
            );
            continue;
        }

        if token.len() < 10 {
            eprintln!("  ✗  That looks too short for a valid token — check and retry.");
            continue;
        }

        println!("  ✓  Token accepted: {}", mask(&token));
        return Ok((token, false));
    }
}

fn step_aw_url() -> Result<String> {
    println!("\nStep 3 of 4 — ActivityWatch URL");
    println!("  Almost always http://localhost:5600. Press Enter to accept.");
    let url = prompt_with_default("  ActivityWatch URL", "http://localhost:5600")?;
    Ok(url.trim_end_matches('/').to_string())
}

/// Returns `(window_bucket, optional_afk_bucket)`. Tries to discover both via
/// the live AW HTTP API and falls back to a hostname-based guess when AW
/// isn't reachable.
fn step_buckets(aw_url: &str) -> Result<(String, Option<String>)> {
    println!("\nStep 4 of 4 — ActivityWatch buckets");

    let discovered = discover_buckets(aw_url);
    let host = detect_hostname();

    let window_bucket = match &discovered {
        Ok(buckets) if !buckets.window.is_empty() => {
            pick_bucket("window-watcher", &buckets.window, &host, "aw-watcher-window")?
        }
        Ok(_) => {
            println!("  ⚠  No window-watcher bucket discovered, falling back to a guess.");
            let guess = format!("aw-watcher-window_{host}");
            prompt_with_default("  Window bucket", &guess)?
        }
        Err(e) => {
            println!("  ⚠  Could not reach ActivityWatch at {aw_url} ({e}).");
            println!("     You'll need to confirm the bucket name manually.");
            let guess = format!("aw-watcher-window_{host}");
            prompt_with_default("  Window bucket", &guess)?
        }
    };
    println!("  ✓  Window bucket: {window_bucket}");

    let afk_bucket = match &discovered {
        Ok(buckets) if !buckets.afk.is_empty() => {
            let pick = pick_bucket_optional("AFK watcher", &buckets.afk, &host, "aw-watcher-afk")?;
            if pick.is_none() {
                println!(
                    "  ⚠  AFK bucket skipped — time you're away from the keyboard \
                     will still be billed. You can add `afk_bucket` to config.toml later."
                );
            }
            pick
        }
        _ => {
            println!();
            println!("  Recommended: enable AFK filtering so time when you're away from");
            println!("  the keyboard is not billed to Kimai.");
            let guess = format!("aw-watcher-afk_{host}");
            let raw = prompt_with_default("  AFK bucket (blank to skip)", &guess)?;
            if raw.trim().is_empty() { None } else { Some(raw) }
        }
    };

    if let Some(b) = &afk_bucket {
        println!("  ✓  AFK bucket: {b}");
    }

    Ok((window_bucket, afk_bucket))
}

// ── Bucket discovery via AW API ───────────────────────────────────────────

struct DiscoveredBuckets {
    window: Vec<String>,
    afk: Vec<String>,
}

#[derive(Deserialize)]
struct ApiBucket {
    #[serde(default, rename = "type")]
    kind: String,
}

/// Hit `GET /api/0/buckets/` on the AW server. We use a short timeout so a
/// stale URL doesn't make the wizard feel hung.
fn discover_buckets(aw_url: &str) -> Result<DiscoveredBuckets> {
    let url = format!("{}/api/0/buckets/", aw_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .context("Failed to build HTTP client")?;
    let resp = client.get(&url).send().context("Failed to call ActivityWatch")?;
    if !resp.status().is_success() {
        anyhow::bail!("AW returned HTTP {}", resp.status());
    }
    let map: HashMap<String, ApiBucket> = resp.json().context("Bad JSON from AW")?;

    let mut window = Vec::new();
    let mut afk = Vec::new();
    for (id, meta) in map {
        let id_lower = id.to_lowercase();
        let kind_lower = meta.kind.to_lowercase();
        if id_lower.contains("window") || kind_lower.contains("window") {
            window.push(id);
        } else if id_lower.contains("afk") || kind_lower.contains("afk") {
            afk.push(id);
        }
    }
    window.sort();
    afk.sort();
    Ok(DiscoveredBuckets { window, afk })
}

/// Required bucket pick: refuse blank, default to whichever choice best
/// matches `hostname`.
fn pick_bucket(kind: &str, options: &[String], host: &str, prefix: &str) -> Result<String> {
    let default = default_for_host(options, host, prefix);
    if options.len() == 1 {
        let only = options[0].clone();
        println!("  Discovered {kind} bucket: {only}");
        return Ok(only);
    }
    println!("  Multiple {kind} buckets found:");
    for (i, b) in options.iter().enumerate() {
        let marker = if Some(b) == default.as_ref() { " (default)" } else { "" };
        println!("    [{}] {}{}", i + 1, b, marker);
    }
    let default_label = default.clone().unwrap_or_else(|| options[0].clone());
    let raw = prompt_with_default("  Pick a number or paste a bucket name", &default_label)?;
    Ok(resolve_choice(&raw, options, &default_label))
}

/// Optional bucket pick: blank = skip.
fn pick_bucket_optional(
    kind: &str,
    options: &[String],
    host: &str,
    prefix: &str,
) -> Result<Option<String>> {
    let default = default_for_host(options, host, prefix);
    if options.len() == 1 {
        let only = options[0].clone();
        println!("  Discovered {kind} bucket: {only}");
        let raw = prompt_with_default("  Use it? [Y/n or paste another, blank to skip]", "Y")?;
        let t = raw.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("y") {
            return Ok(Some(only));
        }
        if t.eq_ignore_ascii_case("n") {
            return Ok(None);
        }
        return Ok(Some(t.to_string()));
    }
    println!("  Multiple {kind} buckets found:");
    for (i, b) in options.iter().enumerate() {
        let marker = if Some(b) == default.as_ref() { " (default)" } else { "" };
        println!("    [{}] {}{}", i + 1, b, marker);
    }
    let default_label = default.clone().unwrap_or_else(|| options[0].clone());
    let raw = prompt_with_default(
        "  Pick a number, paste a name, or blank to skip",
        &default_label,
    )?;
    let t = raw.trim();
    if t.is_empty() {
        return Ok(None);
    }
    Ok(Some(resolve_choice(t, options, &default_label)))
}

fn resolve_choice(raw: &str, options: &[String], default_label: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return default_label.to_string();
    }
    if let Ok(n) = trimmed.parse::<usize>() {
        if n >= 1 && n <= options.len() {
            return options[n - 1].clone();
        }
    }
    trimmed.to_string()
}

fn default_for_host(options: &[String], host: &str, prefix: &str) -> Option<String> {
    let expected = format!("{prefix}_{host}");
    options
        .iter()
        .find(|b| b.eq_ignore_ascii_case(&expected))
        .cloned()
        .or_else(|| options.first().cloned())
}

// ── Config renderer ───────────────────────────────────────────────────────

fn render_config(
    kimai_url: &str,
    kimai_token: &str,
    token_in_env: bool,
    aw_url: &str,
    bucket: &str,
    afk_bucket: Option<&str>,
) -> String {
    let token_line = if token_in_env {
        "# token left blank — supplied via the KIMAI_TOKEN env var\n# token = \"\"".to_string()
    } else {
        format!("token = \"{kimai_token}\"")
    };

    let afk_line = match afk_bucket {
        Some(b) => format!("afk_bucket = \"{b}\""),
        None => "# afk_bucket = \"aw-watcher-afk_YOURHOST\"  # uncomment to ignore away-from-keyboard time".to_string(),
    };

    format!(
        r#"# aw-kimai-bridge configuration
# Generated by `aw-kimai-admin setup`

[kimai]
url   = "{kimai_url}"
{token_line}

# Fallback project/activity used when no [[rules]] pattern matches.
# Run `aw-kimai-admin project list` to find your IDs.
default_project_id  = 0   # ← fill in
default_activity_id = 0   # ← fill in

# Optional: pin entries to a specific Kimai user (omit to use the token owner)
# user_id = 1

[activitywatch]
url    = "{aw_url}"
bucket = "{bucket}"
{afk_line}

# How often the daemon wakes up to sync (seconds). Default: 300 (5 min).
sync_interval_secs = 300

# Gaps shorter than this between same-project events are bridged (seconds).
idle_threshold_secs = 120

# Events shorter than this after merging are silently dropped (seconds).
min_duration_secs = 60

# Where the sync cursor is persisted between runs.
state_path = "state.json"

# ── Classification rules ────────────────────────────────────────────────────
# Rules are tested top-to-bottom; the first match wins.
# Pattern is a case-insensitive regex matched against:  title\napp\nurl
# Set project_id = 0 to explicitly ignore matching events.
#
# [[rules]]
# label      = "My Project"
# pattern    = "myorg/my-project"
# project_id  = 42   # ← from `aw-kimai-admin project list`
# activity_id = 3
#
# [[rules]]
# label      = "Ignore social media"
# pattern    = "twitter\\.com|reddit\\.com|youtube\\.com"
# project_id  = 0
# activity_id = 0
"#
    )
}

// ── Terminal helpers ──────────────────────────────────────────────────────

fn print_banner() {
    println!();
    println!("╔══════════════════════════════════════════╗");
    println!("║      aw-kimai-bridge  ·  Setup Wizard    ║");
    println!("╚══════════════════════════════════════════╝");
    println!();
    println!("This wizard creates a config.toml for the bridge daemon.");
    println!("You can re-run it at any time to regenerate the file.");
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

fn prompt_with_default(label: &str, default: &str) -> Result<String> {
    print!("{label} [{}]: ", default);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let t = buf.trim().to_string();
    Ok(if t.is_empty() { default.to_string() } else { t })
}

/// Try to read without terminal echo (password-style).
/// Falls back to normal readline if the platform doesn't support it.
fn read_secret(label: &str) -> Result<String> {
    #[cfg(unix)]
    {
        if let Ok(token) = read_secret_unix(label) {
            return Ok(token);
        }
    }
    eprintln!("  (note: token will be visible — consider piping input for security)");
    prompt(label)
}

#[cfg(unix)]
fn read_secret_unix(label: &str) -> Result<String> {
    let stdin_fd = {
        use std::os::unix::io::AsRawFd;
        io::stdin().as_raw_fd()
    };

    let mut termios = unsafe {
        let mut t = std::mem::zeroed::<libc::termios>();
        if libc::tcgetattr(stdin_fd, &mut t) != 0 {
            anyhow::bail!("tcgetattr failed");
        }
        t
    };
    let saved = termios;

    termios.c_lflag &= !(libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL);
    unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &termios) };

    print!("{label}: ");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    println!();

    unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &saved) };

    Ok(buf.trim().to_string())
}

fn detect_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "localhost".to_string())
}

fn mask(s: &str) -> String {
    let visible = 4.min(s.len());
    format!("{}…{}", &s[..visible], "*".repeat(6))
}
