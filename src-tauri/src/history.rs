//! Local, on-disk history of usage samples.
//!
//! Samples are grouped into one JSON file per day (`usage-YYYY-MM-DD.json`)
//! under the OS-appropriate app-local-data directory, resolved through
//! Tauri's path API so it lands in the right place on every OS without any
//! platform-specific code here:
//!
//! - Windows: `%LOCALAPPDATA%\<identifier>\history\`
//! - Linux:   `~/.local/share/<identifier>/history/`
//! - macOS:   `~/Library/Application Support/<identifier>/history/`
//!
//! Writes are crash-safe: each update serializes the whole day's samples to
//! a temp file in the same directory, then renames it over the target.
//! Rename is atomic on both Windows and POSIX filesystems, so a crash or
//! power loss mid-write can never leave a half-written/corrupted daily file
//! behind — worst case, the last unrecorded sample is lost.

use crate::codex::UsageSnapshot;
use chrono::{Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const HISTORY_DIR: &str = "history";
pub const DEFAULT_RETENTION_DAYS: i64 = 90;

#[derive(Serialize, Deserialize, Clone)]
pub struct UsageSample {
    /// RFC 3339 timestamp of when this sample was taken (not the reset time).
    pub observed_at: String,
    pub remaining_percent: f32,
    pub reset_at: String,
}

#[derive(Serialize, Deserialize, Default)]
struct DailyLog {
    samples: Vec<UsageSample>,
}

fn history_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("could not resolve app local data dir: {e}"))?;
    let dir = base.join(HISTORY_DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("could not create history dir: {e}"))?;
    Ok(dir)
}

fn file_path_for(dir: &Path, date: NaiveDate) -> PathBuf {
    dir.join(format!("usage-{}.json", date.format("%Y-%m-%d")))
}

fn read_daily_log(path: &Path) -> DailyLog {
    let Ok(text) = fs::read_to_string(path) else {
        return DailyLog::default();
    };
    // A corrupted/partial file (e.g. from an old version of the app, or
    // manual editing) shouldn't crash recording — start fresh for today
    // rather than losing all future samples too.
    serde_json::from_str(&text).unwrap_or_default()
}

fn write_daily_log_atomic(path: &Path, log: &DailyLog) -> Result<(), String> {
    let json = serde_json::to_string_pretty(log).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, json.as_bytes())
        .map_err(|e| format!("failed writing temp history file: {e}"))?;
    fs::rename(&tmp_path, path).map_err(|e| format!("failed to finalize history file: {e}"))?;
    Ok(())
}

/// Appends a sample to today's daily file.
pub fn record_sample(app: &AppHandle, snapshot: &UsageSnapshot) -> Result<(), String> {
    let dir = history_dir(app)?;
    let path = file_path_for(&dir, Utc::now().date_naive());

    let mut log = read_daily_log(&path);
    log.samples.push(UsageSample {
        observed_at: Utc::now().to_rfc3339(),
        remaining_percent: snapshot.remaining_percent,
        reset_at: snapshot.reset_at.clone(),
    });

    write_daily_log_atomic(&path, &log)
}

/// Reads every stored sample across all daily files, oldest first.
/// Used by the pacing calculation in `pacing.rs`.
pub fn read_all_samples(app: &AppHandle) -> Result<Vec<UsageSample>, String> {
    let dir = history_dir(app)?;
    let mut dated_files: Vec<(NaiveDate, PathBuf)> = fs::read_dir(&dir)
        .map_err(|e| format!("could not read history dir: {e}"))?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            parse_date_from_filename(&path).map(|date| (date, path))
        })
        .collect();
    dated_files.sort_by_key(|(date, _)| *date);

    let mut samples = Vec::new();
    for (_, path) in dated_files {
        samples.extend(read_daily_log(&path).samples);
    }
    Ok(samples)
}

/// Deletes daily files older than `retention_days`. Best-effort: a file that
/// fails to delete (e.g. locked by another process) is skipped rather than
/// aborting the whole cleanup.
pub fn cleanup_old_samples(app: &AppHandle, retention_days: i64) -> Result<(), String> {
    let dir = history_dir(app)?;
    let cutoff = Utc::now().date_naive() - Duration::days(retention_days);

    let entries = fs::read_dir(&dir).map_err(|e| format!("could not read history dir: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(date) = parse_date_from_filename(&path) {
            if date < cutoff {
                let _ = fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

fn parse_date_from_filename(path: &Path) -> Option<NaiveDate> {
    let stem = path.file_stem()?.to_str()?; // e.g. "usage-2026-07-27"
    let date_str = stem.strip_prefix("usage-")?;
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}
