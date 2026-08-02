use crate::{
    discovery,
    models::{
        AppConfig, AppSnapshot, DirectConnectRequest, DiscoveredPrinter, PrinterConfig, SyncSummary,
    },
    protocol,
    storage::AppState,
    sync_engine,
};
use chrono::Utc;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub async fn get_snapshot(state: State<'_, Arc<AppState>>) -> Result<AppSnapshot, String> {
    let config = state.config.read().await.clone();
    let status = state.status.read().await.clone();
    let mut history = state.history.read().await.clone();
    history.reverse();
    history.truncate(100);
    Ok(AppSnapshot {
        config,
        status,
        history,
    })
}

#[tauri::command]
pub async fn discover_printers() -> Result<Vec<DiscoveredPrinter>, String> {
    tokio::task::spawn_blocking(|| discovery::discover(Duration::from_secs(4)))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn connect_direct(
    request: DirectConnectRequest,
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<PrinterConfig, String> {
    let printer = PrinterConfig {
        id: format!("http-{}", request.host.trim()),
        name: request
            .name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Snapmaker U1".into()),
        host: request.host.trim().to_string(),
        http_port: request.http_port.unwrap_or(7125),
        sn: request.host.trim().to_string(),
        machine_type: "Snapmaker U1".into(),
        paired: true,
    };
    protocol::verify_http_connection(&printer)
        .await
        .map_err(|error| {
            format!("Moonraker did not respond at the provided IP address: {error}")
        })?;
    {
        let mut config = state.config.write().await;
        config.printer = Some(printer.clone());
    }
    state
        .persist_config()
        .await
        .map_err(|error| error.to_string())?;
    let _ = app.emit("printer-paired", printer.clone());
    Ok(printer)
}

#[tauri::command]
pub async fn save_config(
    mut config: AppConfig,
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<AppConfig, String> {
    config.interval_value = config.interval_value.clamp(1, 8_760);
    if config.version == 0 {
        config.version = 1;
    }
    if !config.destination.trim().is_empty() {
        let destination = std::path::PathBuf::from(config.destination.trim());
        tokio::fs::create_dir_all(&destination)
            .await
            .map_err(|error| format!("could not prepare the folder: {error}"))?;
        config.destination = destination.to_string_lossy().to_string();
    }
    let autostart = app.autolaunch();
    let autostart_enabled = autostart.is_enabled().unwrap_or(false);
    if config.autostart && !autostart_enabled {
        autostart.enable().map_err(|error| error.to_string())?;
    } else if !config.autostart && autostart_enabled {
        autostart.disable().map_err(|error| error.to_string())?;
    }

    {
        *state.config.write().await = config.clone();
        let mut status = state.status.write().await;
        status.next_sync = config.next_sync_from(Utc::now());
        let _ = app.emit("sync-status", status.clone());
    }
    state
        .persist_config()
        .await
        .map_err(|error| error.to_string())?;
    Ok(config)
}

#[tauri::command]
pub async fn pause_current_sync(
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<(), String> {
    let status = {
        let mut status = state.status.write().await;
        if !status.running {
            return Err("no sync is currently running".into());
        }
        state.stop_requested.store(true, Ordering::Release);
        status.phase = "stopping".into();
        status.clone()
    };
    let _ = app.emit("sync-status", status);
    Ok(())
}

#[tauri::command]
pub async fn sync_now(
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<SyncSummary, String> {
    sync_engine::run_sync(state.inner().clone(), app, true)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn verify_printer(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let printer = state
        .config
        .read()
        .await
        .printer
        .clone()
        .ok_or_else(|| "no printer is configured".to_string())?;
    protocol::verify_http_connection(&printer)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn forget_printer(state: State<'_, Arc<AppState>>, app: AppHandle) -> Result<(), String> {
    state.config.write().await.printer = None;
    state
        .persist_config()
        .await
        .map_err(|error| error.to_string())?;
    let _ = app.emit("printer-forgotten", ());
    Ok(())
}

#[tauri::command]
pub async fn clear_history(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.history.write().await.clear();
    state
        .persist_history()
        .await
        .map_err(|error| error.to_string())
}

pub fn spawn_scheduler(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        {
            let config = state.config.read().await.clone();
            let mut status = state.status.write().await;
            status.phase = "idle".into();
            status.next_sync = config.next_sync_from(Utc::now());
            let _ = app.emit("sync-status", status.clone());
        }

        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let config = state.config.read().await.clone();
            if !config.schedule_enabled
                || config.printer.is_none()
                || config.destination.trim().is_empty()
            {
                continue;
            }
            let due = {
                let status = state.status.read().await;
                !status.running
                    && status
                        .next_sync
                        .map(|next| next <= Utc::now())
                        .unwrap_or(true)
            };
            if due {
                if let Err(error) = sync_engine::run_sync(state.clone(), app.clone(), false).await {
                    tracing::error!(error = %error, "scheduled sync failed");
                }
            }
        }
    });
}
