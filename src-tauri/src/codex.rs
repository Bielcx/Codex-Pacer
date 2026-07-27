use serde::Serialize;
use std::process::Command;

#[derive(Serialize)]
pub struct UsageSnapshot {
    pub remaining_percent: f32,
    pub reset_at: String,
    pub source: String,
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
                if let Some(first_line) = text.lines().next() {
                    let trimmed = first_line.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
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
/// TODO: this is a stub. The real implementation needs to speak the
/// codex app-server protocol (confirm whether it's JSON-RPC over stdio,
/// a local socket, or something else) instead of returning fixed values.
pub fn read_usage() -> Result<UsageSnapshot, String> {
    let binary = find_codex_binary()
        .ok_or_else(|| "codex CLI not found. Install it and make sure it's on PATH.".to_string())?;

    Ok(UsageSnapshot {
        remaining_percent: 100.0,
        reset_at: "unknown".to_string(),
        source: binary,
    })
}
