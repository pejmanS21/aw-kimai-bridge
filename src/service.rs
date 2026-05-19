//! Cross-platform "install as a per-user auto-start service" helper.
//!
//! Goal: make the bridge easy to install for non-technical users so they
//! never need to keep a terminal open or remember to start the daemon.
//!
//! Strategy per OS:
//! - **macOS**: write a `LaunchAgent` plist to `~/Library/LaunchAgents` and
//!   load it with `launchctl`.
//! - **Linux**: write a `systemd --user` unit to `~/.config/systemd/user` and
//!   enable+start it with `systemctl --user`. (systemd is effectively
//!   universal on modern desktop Linux; we error clearly if it isn't present.)
//! - **Windows**: drop a `.cmd` shim in the user's Startup folder so the
//!   daemon launches at login. True Windows Services require Admin rights to
//!   install, which we don't want to demand.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const SERVICE_NAME: &str = "aw-kimai-bridge";

// ── Public entry points ────────────────────────────────────────────────────

pub fn install(config_path: &str) -> Result<()> {
    let exe = current_exe()?;
    let abs_config = absolutize(config_path)?;

    if !abs_config.exists() {
        anyhow::bail!(
            "Config file not found: {}\n\
             Create one first with: aw-kimai-admin setup",
            abs_config.display()
        );
    }

    println!("Installing {SERVICE_NAME} as an auto-start service");
    println!("  binary: {}", exe.display());
    println!("  config: {}", abs_config.display());

    let path = platform::install(&exe, &abs_config)?;

    println!();
    println!("✓ Service installed at {}", path.display());
    println!("  It will start now and again automatically on every login.");
    println!("  To uninstall:  {SERVICE_NAME} uninstall-service");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let path = platform::uninstall()?;
    println!("✓ Service removed ({})", path.display());
    Ok(())
}

pub fn status() -> Result<()> {
    match platform::installed_path() {
        Some(p) if p.exists() => {
            println!("Service installed at: {}", p.display());
        }
        _ => {
            println!("Service not installed.");
            println!("Install with:  {SERVICE_NAME} install-service");
        }
    }
    Ok(())
}

// ── Shared helpers ─────────────────────────────────────────────────────────

fn current_exe() -> Result<PathBuf> {
    std::env::current_exe()
        .context("Failed to determine current executable path")?
        .canonicalize()
        .context("Failed to canonicalize executable path")
}

fn absolutize(p: &str) -> Result<PathBuf> {
    let pb = PathBuf::from(p);
    if pb.is_absolute() {
        Ok(pb)
    } else {
        Ok(std::env::current_dir()
            .context("Failed to read current directory")?
            .join(pb))
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .context("Could not determine home directory")
}

// ── Platform-specific implementations ──────────────────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::process::Command;

    pub fn installed_path() -> Option<PathBuf> {
        plist_path().ok()
    }

    pub fn install(exe: &Path, config: &Path) -> Result<PathBuf> {
        let path = plist_path()?;
        std::fs::create_dir_all(path.parent().unwrap())
            .with_context(|| format!("Failed to create {}", path.parent().unwrap().display()))?;

        let label = format!("com.aw-kimai-bridge.{}", whoami_safe());
        let contents = plist_xml(&label, exe, config);
        std::fs::write(&path, contents)
            .with_context(|| format!("Failed to write plist to {}", path.display()))?;

        // Best-effort: unload previous version first, ignore errors.
        let _ = Command::new("launchctl").args(["unload", &path.to_string_lossy()]).status();

        let status = Command::new("launchctl")
            .args(["load", "-w", &path.to_string_lossy()])
            .status()
            .context("Failed to run launchctl. Is it on your PATH?")?;
        if !status.success() {
            anyhow::bail!("launchctl load returned non-zero exit");
        }

        Ok(path)
    }

    pub fn uninstall() -> Result<PathBuf> {
        let path = plist_path()?;
        if path.exists() {
            let _ = Command::new("launchctl").args(["unload", &path.to_string_lossy()]).status();
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
        Ok(path)
    }

    fn plist_path() -> Result<PathBuf> {
        Ok(home_dir()?.join("Library/LaunchAgents/com.aw-kimai-bridge.plist"))
    }

    fn whoami_safe() -> String {
        std::env::var("USER").unwrap_or_else(|_| "user".to_string())
    }

    fn plist_xml(label: &str, exe: &Path, config: &Path) -> String {
        let log_dir = home_dir().map(|h| h.join("Library/Logs"))
            .unwrap_or_else(|_| PathBuf::from("/tmp"));
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>run</string>
        <string>{config}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log_dir}/aw-kimai-bridge.log</string>
    <key>StandardErrorPath</key>
    <string>{log_dir}/aw-kimai-bridge.log</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
</dict>
</plist>
"#,
            exe = exe.display(),
            config = config.display(),
            log_dir = log_dir.display(),
        )
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::process::Command;

    pub fn installed_path() -> Option<PathBuf> {
        unit_path().ok()
    }

    pub fn install(exe: &Path, config: &Path) -> Result<PathBuf> {
        // Confirm systemctl exists; if not, fail with a clear message instead
        // of writing a unit file that will never get picked up.
        if Command::new("systemctl").arg("--version").output().is_err() {
            anyhow::bail!(
                "systemctl not found. This installer supports systemd-based Linux \
                 distributions. On non-systemd systems, run the daemon manually."
            );
        }

        let path = unit_path()?;
        std::fs::create_dir_all(path.parent().unwrap())
            .with_context(|| format!("Failed to create {}", path.parent().unwrap().display()))?;

        std::fs::write(&path, unit_file(exe, config))
            .with_context(|| format!("Failed to write unit file to {}", path.display()))?;

        for args in [
            vec!["--user", "daemon-reload"],
            vec!["--user", "enable", "--now", SERVICE_NAME],
        ] {
            let status = Command::new("systemctl")
                .args(&args)
                .status()
                .context("Failed to run systemctl")?;
            if !status.success() {
                anyhow::bail!("`systemctl {}` returned non-zero exit", args.join(" "));
            }
        }

        // Tell the user about lingering — without it the service stops at logout.
        println!();
        println!("Note: by default systemd --user services stop when you log out.");
        println!("To keep the bridge running across logout/login, enable lingering:");
        println!("    sudo loginctl enable-linger $USER");

        Ok(path)
    }

    pub fn uninstall() -> Result<PathBuf> {
        let path = unit_path()?;
        if path.exists() {
            let _ = Command::new("systemctl")
                .args(["--user", "disable", "--now", SERVICE_NAME])
                .status();
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
            let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).status();
        }
        Ok(path)
    }

    fn unit_path() -> Result<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                home_dir()
                    .map(|h| h.join(".config"))
                    .unwrap_or_else(|_| PathBuf::from(".config"))
            });
        Ok(base.join("systemd/user").join(format!("{SERVICE_NAME}.service")))
    }

    fn unit_file(exe: &Path, config: &Path) -> String {
        format!(
            "[Unit]\n\
             Description=ActivityWatch → Kimai sync bridge\n\
             After=network-online.target\n\
             \n\
             [Service]\n\
             Type=simple\n\
             ExecStart={exe} run {config}\n\
             Restart=on-failure\n\
             RestartSec=10\n\
             Environment=RUST_LOG=info\n\
             \n\
             [Install]\n\
             WantedBy=default.target\n",
            exe = exe.display(),
            config = config.display(),
        )
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;

    pub fn installed_path() -> Option<PathBuf> {
        cmd_path().ok()
    }

    pub fn install(exe: &Path, config: &Path) -> Result<PathBuf> {
        let path = cmd_path()?;
        std::fs::create_dir_all(path.parent().unwrap())
            .with_context(|| format!("Failed to create {}", path.parent().unwrap().display()))?;

        let contents = format!(
            "@echo off\r\n\
             set RUST_LOG=info\r\n\
             start \"\" /B \"{exe}\" run \"{config}\"\r\n",
            exe = exe.display(),
            config = config.display(),
        );
        std::fs::write(&path, contents)
            .with_context(|| format!("Failed to write {}", path.display()))?;

        // Best-effort: also launch right now so the user doesn't have to log out/in.
        let _ = std::process::Command::new(exe).args(["run"]).arg(config).spawn();

        Ok(path)
    }

    pub fn uninstall() -> Result<PathBuf> {
        let path = cmd_path()?;
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
        Ok(path)
    }

    fn cmd_path() -> Result<PathBuf> {
        let appdata = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .context("APPDATA env var not set")?;
        Ok(appdata
            .join("Microsoft/Windows/Start Menu/Programs/Startup")
            .join(format!("{SERVICE_NAME}.cmd")))
    }
}

// Fallback for any OS we didn't special-case (e.g. *BSD). Compile, but tell
// the user we can't install for them.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod platform {
    use super::*;

    pub fn installed_path() -> Option<PathBuf> { None }

    pub fn install(_exe: &Path, _config: &Path) -> Result<PathBuf> {
        anyhow::bail!(
            "Automatic service install is not supported on this OS. \
             Run the daemon manually or add it to your init system."
        );
    }

    pub fn uninstall() -> Result<PathBuf> {
        anyhow::bail!("Nothing to uninstall on this OS.");
    }
}
