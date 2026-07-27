mod app_server;
mod codex;
mod history;

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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_usage])
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
