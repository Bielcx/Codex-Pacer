//! Claude Code rate-limit integration.
//!
//! Unlike Codex, Claude Code has no app-server RPC for rate limits - the
//! only place session/weekly usage percentages show up is the JSON that
//! Claude Code pipes to its `statusLine` hook command on stdin. So instead
//! of talking to Claude Code directly, Codex Pacer plants a small Node hook
//! script (`claude-statusline-hook.cjs`) that Claude Code invokes on its own
//! schedule; the hook writes that payload to a local JSON file
//! (`~/.codex-pacer/claude_status.json`), which `read_status` then reads.
//!
//! This is opt-in (`setup`/`unsetup`) and touches nothing but plain JSON
//! files on disk - no OAuth, no cookies, no credential-store access,
//! consistent with the project's privacy stance (see README.md).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const STATE_DIR_NAME: &str = ".codex-pacer";
const HOOK_SCRIPT_NAME: &str = "claude-statusline-hook.cjs";
const STATE_FILE_NAME: &str = "claude_status.json";
const BACKUP_FILE_NAME: &str = "claude_statusline_backup.json";

/// Written by `setup` into `~/.codex-pacer/`. Reads Claude Code's statusLine
/// JSON from stdin, saves it to `claude_status.json`, and still prints a
/// compact status line so the user's terminal doesn't go blank while Codex
/// Pacer is "docked" onto it. Node is a safe bet as the interpreter: it's
/// already a prerequisite for the most common Claude Code install path (npm).
const HOOK_SCRIPT: &str = r#"#!/usr/bin/env node
// Written by Codex Pacer - captures Claude Code's statusLine payload to a
// local file so Codex Pacer can read session/weekly rate-limit usage.
// Safe to delete; re-run Codex Pacer's Claude setup to restore it.
const fs = require("fs");
const os = require("os");
const path = require("path");

const STATE_DIR = path.join(os.homedir(), ".codex-pacer");
const STATE_FILE = path.join(STATE_DIR, "claude_status.json");

let raw = "";
process.stdin.on("data", (chunk) => { raw += chunk; });
process.stdin.on("end", () => {
  let payload = {};
  try {
    payload = JSON.parse(raw);
  } catch {
    // Malformed/empty stdin - nothing to record.
  }

  try {
    fs.mkdirSync(STATE_DIR, { recursive: true });
    const tmpFile = STATE_FILE + ".tmp";
    fs.writeFileSync(
      tmpFile,
      JSON.stringify({ captured_at: new Date().toISOString(), payload }, null, 2)
    );
    fs.renameSync(tmpFile, STATE_FILE);
  } catch {
    // Best-effort - a write failure shouldn't blank the status line.
  }

  const modelName = (payload.model && payload.model.display_name) || "Claude";
  const fiveHour = payload.rate_limits && payload.rate_limits.five_hour;
  const usage = fiveHour ? ` · ${Math.round(fiveHour.used_percentage)}% used (5h)` : "";
  process.stdout.write(modelName + usage);
});
"#;

pub fn find_claude_binary() -> Option<String> {
    let fallback_paths: Vec<&str> = if cfg!(target_os = "windows") {
        vec![]
    } else if cfg!(target_os = "macos") {
        vec!["/opt/homebrew/bin/claude", "/usr/local/bin/claude"]
    } else {
        vec!["/usr/local/bin/claude", "/usr/bin/claude"]
    };

    crate::cli_finder::find_cli_binary("claude", &fallback_paths)
}

#[derive(Serialize, Deserialize)]
struct StatusLineBackup {
    had_previous: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
}

fn write_json_atomic(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, json.as_bytes()).map_err(|e| format!("failed writing temp file: {e}"))?;
    fs::rename(&tmp_path, path)
        .map_err(|e| format!("failed to finalize {}: {e}", path.display()))?;
    Ok(())
}

fn state_dir_under(home: &Path) -> Result<PathBuf, String> {
    let dir = home.join(STATE_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Plants the hook script under `~/.codex-pacer/` and points
/// `~/.claude/settings.json`'s `statusLine` at it, backing up whatever was
/// there before (including "nothing") on the first run only - re-running
/// setup must not overwrite a real backup with our own config.
fn setup_under(home: &Path) -> Result<(), String> {
    let dir = state_dir_under(home)?;
    let hook_path = dir.join(HOOK_SCRIPT_NAME);
    fs::write(&hook_path, HOOK_SCRIPT).map_err(|e| format!("failed to write hook script: {e}"))?;

    let settings_path = home.join(".claude").join("settings.json");
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }

    let mut settings: serde_json::Value = match fs::read_to_string(&settings_path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("~/.claude/settings.json is not valid JSON: {e}"))?,
        Err(_) => serde_json::json!({}),
    };
    let settings_obj = settings
        .as_object_mut()
        .ok_or_else(|| "~/.claude/settings.json is not a JSON object".to_string())?;

    let backup_path = dir.join(BACKUP_FILE_NAME);
    if !backup_path.exists() {
        let backup = StatusLineBackup {
            had_previous: settings_obj.contains_key("statusLine"),
            value: settings_obj.get("statusLine").cloned(),
        };
        let backup_json = serde_json::to_string_pretty(&backup).map_err(|e| e.to_string())?;
        fs::write(&backup_path, backup_json).map_err(|e| format!("failed to write backup: {e}"))?;
    }

    // Windows: statusLine commands run through a Git-Bash-like shell where
    // backslashes are escape characters, so forward slashes are required.
    let hook_command = format!(
        "node \"{}\"",
        hook_path.display().to_string().replace('\\', "/")
    );
    settings_obj.insert(
        "statusLine".to_string(),
        serde_json::json!({
            "type": "command",
            "command": hook_command,
            "refreshInterval": 30
        }),
    );

    write_json_atomic(&settings_path, &settings)
}

/// Restores whatever `statusLine` value `setup` backed up (or removes the
/// key entirely if there wasn't one), then removes Codex Pacer's own files.
/// A no-op (not an error) if `setup` was never run.
fn unsetup_under(home: &Path) -> Result<(), String> {
    let dir = home.join(STATE_DIR_NAME);
    let backup_path = dir.join(BACKUP_FILE_NAME);

    if let Ok(text) = fs::read_to_string(&backup_path) {
        let backup: StatusLineBackup = serde_json::from_str(&text)
            .map_err(|e| format!("corrupted Claude statusLine backup: {e}"))?;

        let settings_path = home.join(".claude").join("settings.json");
        if let Ok(existing_text) = fs::read_to_string(&settings_path) {
            let mut settings: serde_json::Value = serde_json::from_str(&existing_text)
                .map_err(|e| format!("~/.claude/settings.json is not valid JSON: {e}"))?;
            if let Some(settings_obj) = settings.as_object_mut() {
                if backup.had_previous {
                    if let Some(value) = backup.value {
                        settings_obj.insert("statusLine".to_string(), value);
                    }
                } else {
                    settings_obj.remove("statusLine");
                }
                write_json_atomic(&settings_path, &settings)?;
            }
        }
    }

    let _ = fs::remove_file(dir.join(HOOK_SCRIPT_NAME));
    let _ = fs::remove_file(dir.join(STATE_FILE_NAME));
    let _ = fs::remove_file(&backup_path);

    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct RateLimitWindow {
    pub remaining_percent: f32,
    pub reset_at: DateTime<Utc>,
}

pub struct ClaudeStatusSnapshot {
    pub installed: bool,
    pub configured: bool,
    pub captured_at: Option<DateTime<Utc>>,
    pub five_hour: Option<RateLimitWindow>,
    pub seven_day: Option<RateLimitWindow>,
}

fn parse_window(rate_limits: Option<&serde_json::Value>, key: &str) -> Option<RateLimitWindow> {
    let window = rate_limits?.get(key)?;
    let used_percentage = window.get("used_percentage")?.as_f64()?;
    let resets_at = window.get("resets_at")?.as_i64()?;
    let reset_at = DateTime::<Utc>::from_timestamp(resets_at, 0)?;
    Some(RateLimitWindow {
        remaining_percent: (100.0 - used_percentage) as f32,
        reset_at,
    })
}

fn read_status_under(home: &Path) -> Result<ClaudeStatusSnapshot, String> {
    let installed = find_claude_binary().is_some();

    let dir = home.join(STATE_DIR_NAME);
    let configured = dir.join(HOOK_SCRIPT_NAME).exists();

    if !configured {
        return Ok(ClaudeStatusSnapshot {
            installed,
            configured,
            captured_at: None,
            five_hour: None,
            seven_day: None,
        });
    }

    let Ok(text) = fs::read_to_string(dir.join(STATE_FILE_NAME)) else {
        // Hook is wired up but hasn't run yet - no Claude Code session yet.
        return Ok(ClaudeStatusSnapshot {
            installed,
            configured,
            captured_at: None,
            five_hour: None,
            seven_day: None,
        });
    };

    let state: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("corrupted Claude status file: {e}"))?;

    let captured_at = state
        .get("captured_at")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let rate_limits = state.get("payload").and_then(|p| p.get("rate_limits"));

    Ok(ClaudeStatusSnapshot {
        installed,
        configured,
        captured_at,
        five_hour: parse_window(rate_limits, "five_hour"),
        seven_day: parse_window(rate_limits, "seven_day"),
    })
}

fn home_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .home_dir()
        .map_err(|e| format!("could not resolve home dir: {e}"))
}

pub fn setup(app: &AppHandle) -> Result<(), String> {
    setup_under(&home_dir(app)?)
}

pub fn unsetup(app: &AppHandle) -> Result<(), String> {
    unsetup_under(&home_dir(app)?)
}

pub fn read_status(app: &AppHandle) -> Result<ClaudeStatusSnapshot, String> {
    read_status_under(&home_dir(app)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh scratch "home" dir per test, cleaned up on drop. Keeps these
    /// tests from ever touching a real `~/.claude/settings.json`.
    struct TempHome(PathBuf);

    impl TempHome {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "codex-pacer-claude-test-{n}-{}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            TempHome(dir)
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_state_file(home: &Path, json: serde_json::Value) {
        let dir = home.join(STATE_DIR_NAME);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(STATE_FILE_NAME),
            serde_json::to_string(&json).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn not_configured_before_setup() {
        let home = TempHome::new();
        let status = read_status_under(&home.0).unwrap();
        assert!(!status.configured);
        assert!(status.captured_at.is_none());
    }

    #[test]
    fn setup_writes_hook_and_points_settings_at_it() {
        let home = TempHome::new();
        setup_under(&home.0).unwrap();

        assert!(home.0.join(STATE_DIR_NAME).join(HOOK_SCRIPT_NAME).exists());

        let settings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(home.0.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["statusLine"]["type"], "command");
        assert!(settings["statusLine"]["command"]
            .as_str()
            .unwrap()
            .contains(HOOK_SCRIPT_NAME));

        let status = read_status_under(&home.0).unwrap();
        assert!(status.configured);
    }

    #[test]
    fn setup_backs_up_existing_status_line_once() {
        let home = TempHome::new();
        fs::create_dir_all(home.0.join(".claude")).unwrap();
        fs::write(
            home.0.join(".claude/settings.json"),
            r#"{"statusLine": {"type": "command", "command": "my-old-script.sh"}}"#,
        )
        .unwrap();

        setup_under(&home.0).unwrap();
        // Re-running setup (e.g. app restart) must not clobber the backup
        // with our own statusLine from the first run.
        setup_under(&home.0).unwrap();

        unsetup_under(&home.0).unwrap();

        let settings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(home.0.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["statusLine"]["command"], "my-old-script.sh");
    }

    #[test]
    fn unsetup_removes_status_line_when_none_existed_before() {
        let home = TempHome::new();
        setup_under(&home.0).unwrap();
        unsetup_under(&home.0).unwrap();

        let settings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(home.0.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert!(settings.get("statusLine").is_none());
        assert!(!home.0.join(STATE_DIR_NAME).join(HOOK_SCRIPT_NAME).exists());
    }

    #[test]
    fn unsetup_without_setup_is_a_harmless_no_op() {
        let home = TempHome::new();
        assert!(unsetup_under(&home.0).is_ok());
    }

    #[test]
    fn reads_rate_limit_windows_when_present() {
        let home = TempHome::new();
        fs::create_dir_all(home.0.join(STATE_DIR_NAME)).unwrap();
        fs::write(home.0.join(STATE_DIR_NAME).join(HOOK_SCRIPT_NAME), "").unwrap();
        write_state_file(
            &home.0,
            serde_json::json!({
                "captured_at": Utc::now().to_rfc3339(),
                "payload": {
                    "rate_limits": {
                        "five_hour": { "used_percentage": 23.5, "resets_at": 1_738_425_600 },
                        "seven_day": { "used_percentage": 41.2, "resets_at": 1_738_857_600 }
                    }
                }
            }),
        );

        let status = read_status_under(&home.0).unwrap();
        assert!(status.configured);
        assert!(status.captured_at.is_some());
        let five_hour = status.five_hour.unwrap();
        assert!((five_hour.remaining_percent - 76.5).abs() < 0.01);
        assert!(status.seven_day.is_some());
    }

    #[test]
    fn no_rate_limits_field_reads_as_api_key_billing() {
        let home = TempHome::new();
        fs::create_dir_all(home.0.join(STATE_DIR_NAME)).unwrap();
        fs::write(home.0.join(STATE_DIR_NAME).join(HOOK_SCRIPT_NAME), "").unwrap();
        write_state_file(
            &home.0,
            serde_json::json!({
                "captured_at": Utc::now().to_rfc3339(),
                "payload": { "model": { "display_name": "Claude" } }
            }),
        );

        let status = read_status_under(&home.0).unwrap();
        assert!(status.configured);
        assert!(status.captured_at.is_some());
        assert!(status.five_hour.is_none());
        assert!(status.seven_day.is_none());
    }
}
