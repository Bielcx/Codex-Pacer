use crate::app_server::AppServerClient;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};

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

#[derive(Serialize)]
pub struct CostSummary {
    pub today_tokens: u64,
    pub last_30_days_tokens: u64,
}

/// Locates the `codex` CLI executable.
///
/// Primary install path today is npm (`npm i -g @openai/codex`), which puts
/// the binary on PATH on every OS, so PATH lookup is tried first. A short
/// list of well-known fallback locations covers older/manual installs.
pub fn find_codex_binary() -> Option<String> {
    let fallback_paths: Vec<&str> = if cfg!(target_os = "windows") {
        vec![]
    } else if cfg!(target_os = "macos") {
        vec!["/opt/homebrew/bin/codex", "/usr/local/bin/codex"]
    } else {
        vec!["/usr/local/bin/codex", "/usr/bin/codex"]
    };

    crate::cli_finder::find_cli_binary("codex", &fallback_paths)
}

/// Reads a usage snapshot from the codex CLI app-server.
///
/// Spawns its own `codex app-server` process. Prefer `read_usage_and_cost`
/// when both usage and cost are needed in the same refresh, to avoid paying
/// for two separate spawns + handshakes.
pub fn read_usage() -> Result<UsageSnapshot, String> {
    let binary = find_codex_binary()
        .ok_or_else(|| "codex CLI not found. Install it and make sure it's on PATH.".to_string())?;

    let mut client = AppServerClient::spawn(&binary)?;
    let result = client.call("account/rateLimits/read", json!({}))?;
    parse_usage_snapshot(&result, binary)
}

/// Spawns a single `codex app-server` process and issues both the
/// rate-limit and token-usage calls the popover's Codex tab needs, instead
/// of two separate spawns (each with its own process start + handshake).
/// The double spawn was adding a visible stutter to every auto-refresh.
pub fn read_usage_and_cost() -> Result<(UsageSnapshot, CostSummary), String> {
    let binary = find_codex_binary()
        .ok_or_else(|| "codex CLI not found. Install it and make sure it's on PATH.".to_string())?;

    let mut client = AppServerClient::spawn(&binary)?;

    let usage_result = client.call("account/rateLimits/read", json!({}))?;
    let usage = parse_usage_snapshot(&usage_result, binary.clone())?;

    let cost_result = client.call("account/usage/read", json!({}))?;
    let cost = parse_cost_summary(&cost_result)?;

    Ok((usage, cost))
}

fn parse_usage_snapshot(result: &Value, binary: String) -> Result<UsageSnapshot, String> {
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

/// Parses `account/usage/read`'s response. Despite the struct's name, this
/// carries no dollar figures - the confirmed response shape (see
/// `docs/app-server-protocol.md`) only has a per-day token bucket list and a
/// lifetime summary, no per-model pricing breakdown to compute cost from.
fn parse_cost_summary(result: &Value) -> Result<CostSummary, String> {
    let buckets = result
        .get("dailyUsageBuckets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "unexpected response shape from account/usage/read".to_string())?;

    let today = Utc::now().date_naive();
    let window_start = today - chrono::Duration::days(29);

    let mut today_tokens = 0u64;
    let mut last_30_days_tokens = 0u64;

    for bucket in buckets {
        let Some(date) = bucket
            .get("startDate")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        else {
            continue;
        };
        let tokens = bucket.get("tokens").and_then(|v| v.as_u64()).unwrap_or(0);

        if date == today {
            today_tokens = tokens;
        }
        if date >= window_start && date <= today {
            last_30_days_tokens += tokens;
        }
    }

    Ok(CostSummary {
        today_tokens,
        last_30_days_tokens,
    })
}
