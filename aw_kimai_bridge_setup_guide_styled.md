# aw-kimai-bridge Setup Guide

Step-by-step instructions for Windows, macOS, and Linux.

Pick your operating system below. Each section walks you through the full setup — no technical experience needed.

---

## Table of Contents

- [Windows Setup](#windows-setup)
- [macOS Setup](#macos-setup)
- [Linux Setup](#linux-setup)

## Windows Setup

### Step 1 — Download the program

Go to the Releases page on GitHub and download `aw-kimai-bridge-windows-amd64.zip` (use `arm64` if you have a Surface Pro X or similar ARM device).

> **Not sure which one?** Almost everyone needs **amd64**.

---

### Step 2 — Extract and place the files

Right-click the zip file → *Extract All*. Then create this folder and move both `.exe` files into it:

```
C:\aw-kimai-bridge\
```

Your folder should look like this:

```
C:\aw-kimai-bridge\
  aw-kimai-bridge.exe
  aw-kimai-admin.exe
```

---

### Step 3 — Create your config file

Open Notepad, paste the text below, fill in your own values, then save it as `config.toml` inside `C:\aw-kimai-bridge\`.

> ⚠️ **When saving in Notepad**, set "Save as type" to **All Files (*.*)** and type the filename as **config.toml** — otherwise Notepad adds .txt to the end.

**File:** `C:\aw-kimai-bridge\config.toml`

```toml
[kimai]
url                 = "https://your-kimai-site.com"
token               = "paste-your-api-token-here"
default_project_id  = 1
default_activity_id = 1

[activitywatch]
url    = "http://localhost:5600"
bucket = "aw-watcher-window_YOUR-PC-NAME"

sync_interval_secs  = 300
idle_threshold_secs = 120
min_duration_secs   = 60
state_path          = "C:\\aw-kimai-bridge\\state.json"
```

---

### Step 4 — Set it to run automatically on login

Press `Win + R`, type `shell:startup` and press Enter. A folder will open. Create a new file called `start-aw-kimai.bat` in that folder with this content:

**File:** `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\start-aw-kimai.bat`

```bat
@echo off
start "" /B "C:\aw-kimai-bridge\aw-kimai-bridge.exe" "C:\aw-kimai-bridge\config.toml" >> "C:\aw-kimai-bridge\bridge.log" 2>&1
```

> This file runs silently in the background every time you log in. The window is hidden on purpose.

---

### Step 5 — Start it now (without restarting)

Double-click the `start-aw-kimai.bat` file you just created. The bridge is now running. To confirm, check the log file at:

```
C:\aw-kimai-bridge\bridge.log
```

---

### Step 6 — To stop or restart the bridge

Press `Ctrl + Alt + Del` → Task Manager → find `aw-kimai-bridge.exe` → right-click → End Task. It will start again next time you log in.

---

## macOS Setup

### Step 1 — Download the program

Go to the Releases page on GitHub and download the correct file for your Mac:

> **Apple Silicon Mac (M1/M2/M3/M4)?** Download `aw-kimai-bridge-macos-arm64.tar.gz`
>
> **Older Intel Mac?** Download `aw-kimai-bridge-macos-amd64.tar.gz`
>
> Not sure? Click the Apple menu () → About This Mac. If it says "Apple M1" (or M2/M3/M4) choose arm64. If it says "Intel" choose amd64.

---

### Step 2 — Open Terminal

Press `Cmd + Space`, type `Terminal`, press Enter. A black or white window will appear — this is normal. You'll paste a few commands into it.

---

### Step 3 — Install the program

Paste these commands one at a time, pressing Enter after each one:

```bash
mkdir -p ~/.config/aw-kimai-bridge
```

```bash
tar -xzf ~/Downloads/aw-kimai-bridge-macos-*.tar.gz -C /tmp
```

```bash
sudo cp /tmp/aw-kimai-bridge-macos-*/aw-kimai-bridge /usr/local/bin/
sudo cp /tmp/aw-kimai-bridge-macos-*/aw-kimai-admin /usr/local/bin/
```

> The `sudo` commands will ask for your Mac password. Nothing will appear as you type — this is normal, just type it and press Enter.

---

### Step 4 — Allow the app to run (Gatekeeper)

Because the app wasn't downloaded from the App Store, macOS will block it the first time. Run this to allow it:

```bash
xattr -dr com.apple.quarantine /usr/local/bin/aw-kimai-bridge /usr/local/bin/aw-kimai-admin
```

---

### Step 5 — Create your config file

Paste this into Terminal to open a text editor with the config file:

```bash
nano ~/.config/aw-kimai-bridge/config.toml
```

The editor will open. Paste the config below, fill in your values, then press `Ctrl+O` → Enter to save, then `Ctrl+X` to exit.

**File:** `~/.config/aw-kimai-bridge/config.toml`

```toml
[kimai]
url                 = "https://your-kimai-site.com"
token               = "paste-your-api-token-here"
default_project_id  = 1
default_activity_id = 1

[activitywatch]
url    = "http://localhost:5600"
bucket = "aw-watcher-window_YOUR-MAC-NAME"

sync_interval_secs  = 300
idle_threshold_secs = 120
min_duration_secs   = 60
state_path          = "/Users/YOUR-USERNAME/.config/aw-kimai-bridge/state.json"
```

---

### Step 6 — Set it to run automatically on login

This creates a Launch Agent — macOS's built-in way to run programs in the background. Paste this whole block into Terminal at once:

```bash
mkdir -p ~/Library/LaunchAgents ~/Library/Logs
cat > ~/Library/LaunchAgents/com.aw-kimai-bridge.plist << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.aw-kimai-bridge</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/aw-kimai-bridge</string>
        <string>/Users/YOUR-USERNAME/.config/aw-kimai-bridge/config.toml</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/YOUR-USERNAME/.local/log/aw-kimai-bridge.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/YOUR-USERNAME/.local/log/aw-kimai-bridge.log</string>
</dict>
</plist>
EOF
```

Then load it:

```bash
launchctl load ~/Library/LaunchAgents/com.aw-kimai-bridge.plist
```

> The bridge is now running and will start automatically every time you log in.

---

### Step 7 — Check the logs

```bash
tail -f ~/.local/log/aw-kimai-bridge.log
```

You should see lines like `Sync cycle complete` appearing every 5 minutes. Press `Ctrl+C` to stop watching.

---

### Step 8 — To stop or restart the bridge

```bash
launchctl unload ~/Library/LaunchAgents/com.aw-kimai-bridge.plist
```

```bash
launchctl load ~/Library/LaunchAgents/com.aw-kimai-bridge.plist
```

---

## Linux Setup

### Step 1 — Download the program

Go to the Releases page and download `aw-kimai-bridge-linux-amd64.tar.gz` (or `arm64` if you're on a Raspberry Pi or ARM device). Then open a Terminal and run:

```bash
mkdir -p ~/.config/aw-kimai-bridge ~/.local/log
```

```bash
tar -xzf ~/Downloads/aw-kimai-bridge-linux-*.tar.gz -C /tmp
```

```bash
sudo cp /tmp/aw-kimai-bridge-linux-*/aw-kimai-bridge /usr/local/bin/
sudo cp /tmp/aw-kimai-bridge-linux-*/aw-kimai-admin /usr/local/bin/
```

---

### Step 2 — Create your config file

```bash
nano ~/.config/aw-kimai-bridge/config.toml
```

Paste the config below, fill in your values, then press `Ctrl+O` → Enter to save, `Ctrl+X` to exit.

**File:** `~/.config/aw-kimai-bridge/config.toml`

```toml
[kimai]
url                 = "https://your-kimai-site.com"
token               = "paste-your-api-token-here"
default_project_id  = 1
default_activity_id = 1

[activitywatch]
url    = "http://localhost:5600"
bucket = "aw-watcher-window_YOUR-HOSTNAME"

sync_interval_secs  = 300
idle_threshold_secs = 120
min_duration_secs   = 60
state_path          = "/home/YOUR-USERNAME/.config/aw-kimai-bridge/state.json"
```

---

### Step 3 — Create the systemd service

This is how Linux runs programs in the background. Paste this whole block into Terminal at once:

```bash
sudo tee /etc/systemd/system/aw-kimai-bridge.service > /dev/null << EOF
[Unit]
Description=ActivityWatch to Kimai time sync bridge
After=network.target

[Service]
Type=simple
User=$USER
WorkingDirectory=$HOME/.config/aw-kimai-bridge
ExecStart=/usr/local/bin/aw-kimai-bridge config.toml
Restart=on-failure
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF
```

---

### Step 4 — Enable and start it

These three commands register it, start it now, and make it start on every boot:

```bash
sudo systemctl daemon-reload
```

```bash
sudo systemctl enable aw-kimai-bridge
```

```bash
sudo systemctl start aw-kimai-bridge
```

---

### Step 5 — Check it's running

```bash
sudo systemctl status aw-kimai-bridge
```

You should see `Active: active (running)` in green. To watch the live log:

```bash
journalctl -u aw-kimai-bridge -f
```

Press `Ctrl+C` to stop watching.

---

### Step 6 — To stop or restart the bridge

```bash
sudo systemctl stop aw-kimai-bridge
```

```bash
sudo systemctl restart aw-kimai-bridge
```