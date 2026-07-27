mod app_server;
mod codex;
mod history;
mod pacing;

use chrono::{DateTime, Duration, Utc};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

#[tauri::command]
fn get_usage(app: tauri::AppHandle) -> Result<codex::UsageSnapshot, String> {
    let snapshot = codex::read_usage().map_err(|e| e.to_string())?;

    // Recording history should never break the UI even if it fails (e.g.
    // disk full, permissions) - log and move on.
    if let Err(e) = history::record_sample(&app, &snapshot) {
        eprintln!("failed to record usage sample: {e}");
    }

    Ok(snapshot)
}

#[derive(serde::Serialize)]
struct PacingResult {
    remaining_percent: f32,
    reset_at: String,
    window_start: String,
    safety_buffer_percent: f32,
    source: String,
    target_remaining_percent: f32,
    safe_burn_rate_per_hour: f32,
    recent_burn_rate_per_hour: Option<f32>,
    verdict: pacing::Verdict,
}

#[tauri::command]
fn get_pacing(app: tauri::AppHandle) -> Result<PacingResult, String> {
    let snapshot = codex::read_usage().map_err(|e| e.to_string())?;

    if let Err(e) = history::record_sample(&app, &snapshot) {
        eprintln!("failed to record usage sample: {e}");
    }
    let samples = history::read_all_samples(&app).unwrap_or_default();

    let reset_at = DateTime::parse_from_rfc3339(&snapshot.reset_at)
        .map_err(|e| format!("invalid reset_at timestamp `{}`: {e}", snapshot.reset_at))?
        .with_timezone(&Utc);
    let window_start = reset_at - Duration::minutes(snapshot.window_duration_mins);

    let report = pacing::evaluate(
        snapshot.remaining_percent,
        window_start,
        reset_at,
        Utc::now(),
        pacing::DEFAULT_SAFETY_BUFFER_PERCENT,
        pacing::DEFAULT_TOLERANCE_PERCENT,
        &samples,
    );

    Ok(PacingResult {
        remaining_percent: snapshot.remaining_percent,
        reset_at: snapshot.reset_at,
        window_start: window_start.to_rfc3339(),
        safety_buffer_percent: pacing::DEFAULT_SAFETY_BUFFER_PERCENT,
        source: snapshot.source,
        target_remaining_percent: report.target_remaining_percent,
        safe_burn_rate_per_hour: report.safe_burn_rate_per_hour,
        recent_burn_rate_per_hour: report.recent_burn_rate_per_hour,
        verdict: report.verdict,
    })
}

#[derive(serde::Serialize)]
struct HistoryPoint {
    observed_at: String,
    remaining_percent: f32,
}

/// All stored samples, for the popover's burn-down chart. The frontend
/// filters down to the current window client-side using the `window_start`
/// it already got from `get_pacing`, so this stays a single flat list
/// instead of needing its own window-lookup (which would mean spawning
/// another `codex app-server` just to draw a chart).
#[tauri::command]
fn get_history(app: tauri::AppHandle) -> Result<Vec<HistoryPoint>, String> {
    let samples = history::read_all_samples(&app)?;
    Ok(samples
        .into_iter()
        .map(|s| HistoryPoint {
            observed_at: s.observed_at,
            remaining_percent: s.remaining_percent,
        })
        .collect())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_usage, get_pacing, get_history])
        .setup(|app| {
            if let Err(e) = history::cleanup_old_samples(&app.handle(), history::DEFAULT_RETENTION_DAYS) {
                eprintln!("failed to clean up old usage history: {e}");
            }

            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit])?;

            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    // `Click` fires once for button-down and once for
                    // button-up; only act on release, otherwise a normal
                    // click shows the window and immediately hides it again.
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let is_visible = window.is_visible().unwrap_or(false);
                            let _ = if is_visible {
                                window.hide()
                            } else {
                                window.show().and_then(|_| window.set_focus())
                            };
                        }
                    }
                })
                .icon(app.default_window_icon().unwrap().clone())
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Codex Pacer");
}
