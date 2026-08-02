mod commands;
mod discovery;
mod models;
mod protocol;
mod storage;
mod sync_engine;

use std::sync::{atomic::Ordering, Arc};
use storage::AppState;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn cleanup_old_logs(log_dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
            .map(|age| age > std::time::Duration::from_secs(14 * 24 * 60 * 60))
            .unwrap_or(false);
        if is_old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let log_dir = data_dir.join("logs");
            std::fs::create_dir_all(&log_dir)?;
            cleanup_old_logs(&log_dir);
            let file_appender = tracing_appender::rolling::daily(log_dir, "syncer.log");
            let _ = tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "snapmaker_timelapse_syncer=info".into()),
                )
                .with_ansi(false)
                .with_writer(file_appender)
                .try_init();

            let state = Arc::new(AppState::load(data_dir)?);
            app.manage(state.clone());

            let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
            let sync = MenuItem::with_id(app, "sync", "Sync now", true, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause", "Pause sync", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &sync, &pause, &quit])?;

            TrayIconBuilder::with_id("main")
                .icon(
                    app.default_window_icon()
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("main application icon not found"))?,
                )
                .tooltip("SnapSync · Timelapses")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main_window(app),
                    "sync" => {
                        let state = app.state::<Arc<AppState>>().inner().clone();
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(error) = sync_engine::run_sync(state, handle, true).await {
                                tracing::error!(error = %error, "manual sync failed");
                            }
                        });
                    }
                    "pause" => {
                        let state = app.state::<Arc<AppState>>().inner().clone();
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let mut status = state.status.write().await;
                            if status.running {
                                state.stop_requested.store(true, Ordering::Release);
                                status.phase = "stopping".into();
                                let _ = handle.emit("sync-status", status.clone());
                            }
                        });
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            commands::spawn_scheduler(app.handle().clone(), state);
            if std::env::args().any(|argument| argument == "--background") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Arc<AppState>>();
                if state.config.blocking_read().run_in_background {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::discover_printers,
            commands::connect_direct,
            commands::save_config,
            commands::pause_current_sync,
            commands::sync_now,
            commands::verify_printer,
            commands::forget_printer,
            commands::clear_history,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
