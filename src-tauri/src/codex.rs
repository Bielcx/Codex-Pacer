use crate::app_server::AppServerClient;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use std::process::Command;

#[derive(Serialize)]
pub struct UsageSnapshot {
    pub remaining_percent: f32,
    pub reset_at: String,
    pub source: String,
    /// Length of the current rate-limit window, in minutes. Needed to work
    /// out when the window started (`reset_at - window_duration_mins`),
    /// which the pacing calculation uses as the start of its target
    /// trajectory.
    pub window_duration_mins: i64,
}

/// Locates the `codex` CLI executable.
///
/// Primary install path today is npm (`npm i -g @openai/codex`), which puts
/// the binary on PATH on every OS, so PATH lookup is tried first. A short
/// list of well-known fallback locations covers older/manual installs.
pub fn find_codex_binary() -> Option<String> {
    let finder = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };

    if let Ok(output) = Command::new(finder).arg("codex").output() {
        if output.status.success() {
            if let Ok(text) = String::from_utf8(output.stdout) {
                let candidates: Vec<&str> = text
                    .lines()
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty())
                    .collect();

                if cfg!(target_os = "windows") {
                    // npm's global installer drops several shims per package
                    // (an extension-less POSIX shell script for Git
                    // Bash/WSL, plus `.cmd`/`.ps1` for native Windows).
                    // `where` lists the extension-less one first because it
                    // matches the bare name exactly, but CreateProcess can't
                    // execute it directly (error 193). Prefer something
                    // Windows can actually run.
                    if let Some(exe) = candidates
                        .iter()
                        .find(|p| p.to_lowercase().ends_with(".exe"))
                    {
                        return Some(exe.to_string());
                    }
                    if let Some(cmd) = candidates.iter().find(|p| {
                        let lower = p.to_lowercase();
                        lower.ends_with(".cmd") || lower.ends_with(".bat")
                    }) {
                        return Some(cmd.to_string());
                    }
                }

                if let Some(first) = candidates.first() {
                    return Some(first.to_string());
                }
            }
        }
    }

    let fallback_paths: Vec<&str> = if cfg!(target_os = "windows") {
        vec![]
    } else if cfg!(target_os = "macos") {
        vec!["/opt/homebrew/bin/codex", "/usr/local/bin/codex"]
    } else {
        vec!["/usr/local/bin/codex", "/usr/bin/codex"]
    };

    fallback_paths
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|s| s.to_string())
}

/// Reads a usage snapshot from the codex CLI app-server.
///
/// Spawns `codex app-server`, performs the initialize handshake, and calls
/// `account/rateLimits/read`. See `docs/app-server-protocol.md` for the full
/// protocol notes.
pub fn read_usage() -> Result<UsageSnapshot, String> {
    let binary = find_codex_binary()
        .ok_or_else(|| "codex CLI not found. Install it and make sure it's on PATH.".to_string())?;

    let mut client = AppServerClient::spawn(&binary)?;
    let result = client.call("account/rateLimits/read", json!({}))?;

    let primary = result
        .get("rateLimits")
        .and_then(|r| r.get("primary"))
        .ok_or_else(|| "unexpected response shape from account/rateLimits/read".to_string())?;

    let used_percent = primary
        .get("usedPercent")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "missing usedPercent in rate limit response".to_string())?;

    let reset_at = primary
        .get("resetsAt")
        .and_then(|v| v.as_i64())
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0))
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "unknown".to_string());

    let window_duration_mins = primary
        .get("windowDurationMins")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    Ok(UsageSnapshot {
        remaining_percent: (100.0 - used_percent) as f32,
        reset_at,
        source: binary,
        window_duration_mins,
    })
}
