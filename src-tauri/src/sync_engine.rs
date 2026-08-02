use crate::{
    models::{AppConfig, HistoryEntry, HistoryResult, SyncStatus, SyncSummary, TimelapseInstance},
    protocol,
    storage::AppState,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{TimeZone, Utc};
use futures_util::StreamExt;
use reqwest::Client;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use url::Url;
use uuid::Uuid;

pub async fn run_sync(
    state: Arc<AppState>,
    app: AppHandle,
    _force: bool,
) -> anyhow::Result<SyncSummary> {
    let _sync_guard = state
        .sync_lock
        .try_lock()
        .map_err(|_| anyhow::anyhow!("a sync is already running"))?;
    let config = state.config.read().await.clone();
    state.stop_requested.store(false, Ordering::Release);

    let printer = config
        .printer
        .clone()
        .filter(|printer| printer.paired)
        .ok_or_else(|| anyhow::anyhow!("connect a printer before syncing"))?;
    if config.destination.trim().is_empty() {
        anyhow::bail!("select a destination folder");
    }

    let destination = PathBuf::from(&config.destination);
    tokio::fs::create_dir_all(&destination)
        .await
        .map_err(|error| anyhow::anyhow!("could not create the destination folder: {error}"))?;
    if !destination.is_dir() {
        anyhow::bail!("the configured destination is not a folder");
    }

    set_status(&state, &app, |status| {
        status.phase = "listing".into();
        status.running = true;
        status.current_file = None;
        status.progress_percent = 0;
        status.downloaded = 0;
        status.skipped = 0;
        status.failed = 0;
        status.last_error = None;
    })
    .await;

    let instances = match protocol::list_camera_files_http(&printer).await {
        Ok(instances) => instances,
        Err(error) => {
            finish_with_error(&state, &app, &config, error.to_string()).await;
            return Err(error);
        }
    };

    let completed_files: HashMap<String, HistoryEntry> = state
        .history
        .read()
        .await
        .iter()
        .filter(|entry| {
            matches!(
                entry.result,
                HistoryResult::Downloaded | HistoryResult::AlreadyPresent
            )
        })
        .map(|entry| (entry.remote_key.clone(), entry.clone()))
        .collect();

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(12))
        .build()?;
    let total = instances.len().max(1);
    let mut summary = SyncSummary {
        downloaded: 0,
        skipped: 0,
        failed: 0,
    };
    let mut stopped = false;

    for (index, instance) in instances.into_iter().enumerate() {
        if should_stop_before_next_file(&state.stop_requested) {
            tracing::info!("sync paused before the next file");
            stopped = true;
            break;
        }
        let remote_key = instance.remote_key(&printer.sn);
        if let Some(entry) = completed_files.get(&remote_key) {
            if local_copy_is_valid(entry, &destination, instance.video_file_size).await {
                summary.skipped += 1;
                update_counts(&state, &app, &summary, index, total, None).await;
                continue;
            }
            tracing::info!(
                file = %entry.local_path,
                "local copy is missing or changed; the timelapse will be downloaded again"
            );
        }

        let display_name = if instance.gcode_name.is_empty() {
            Path::new(&instance.video_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("timelapse")
                .to_string()
        } else {
            instance.gcode_name.clone()
        };
        update_counts(
            &state,
            &app,
            &summary,
            index,
            total,
            Some(display_name.clone()),
        )
        .await;

        match download_instance(
            &client,
            &printer.host,
            printer.http_port,
            &destination,
            &instance,
            &state,
            &app,
        )
        .await
        {
            Ok(download) => {
                if config.download_thumbnails {
                    if let Err(error) = save_thumbnail(
                        &client,
                        &printer.host,
                        printer.http_port,
                        &download.path,
                        &instance,
                    )
                    .await
                    {
                        tracing::warn!(error = %error, "could not save the thumbnail");
                    }
                }

                if download.already_present {
                    summary.skipped += 1;
                } else {
                    summary.downloaded += 1;
                }
                state
                    .add_history(HistoryEntry {
                        id: Uuid::new_v4().to_string(),
                        remote_key,
                        printer_sn: printer.sn.clone(),
                        gcode_name: display_name,
                        remote_path: instance.video_path.clone(),
                        local_path: download.path.to_string_lossy().to_string(),
                        size: download.size,
                        completed_at: Utc::now(),
                        result: if download.already_present {
                            HistoryResult::AlreadyPresent
                        } else {
                            HistoryResult::Downloaded
                        },
                        remote_deleted: false,
                        message: String::new(),
                    })
                    .await?;
            }
            Err(error) => {
                summary.failed += 1;
                tracing::error!(
                    file = %instance.video_path,
                    error = %error,
                    "timelapse download failed"
                );
                state
                    .add_history(HistoryEntry {
                        id: Uuid::new_v4().to_string(),
                        remote_key,
                        printer_sn: printer.sn.clone(),
                        gcode_name: display_name,
                        remote_path: instance.video_path.clone(),
                        local_path: String::new(),
                        size: 0,
                        completed_at: Utc::now(),
                        result: HistoryResult::Failed,
                        remote_deleted: false,
                        message: error.to_string(),
                    })
                    .await?;
                set_status(&state, &app, |status| {
                    status.last_error = Some(error.to_string());
                })
                .await;
            }
        }
        update_counts(&state, &app, &summary, index + 1, total, None).await;
    }

    let now = Utc::now();
    set_status(&state, &app, |status| {
        status.phase = "idle".into();
        status.running = false;
        status.current_file = None;
        if !stopped {
            status.progress_percent = 100;
        }
        status.last_sync = Some(now);
        status.next_sync = config.next_sync_from(now);
    })
    .await;

    Ok(summary)
}

fn should_stop_before_next_file(stop_requested: &AtomicBool) -> bool {
    stop_requested.load(Ordering::Acquire)
}

async fn local_copy_is_valid(entry: &HistoryEntry, destination: &Path, expected_size: u64) -> bool {
    if entry.local_path.trim().is_empty() {
        return false;
    }
    let local_path = PathBuf::from(&entry.local_path);
    if !local_path.starts_with(destination) {
        return false;
    }
    let Ok(metadata) = tokio::fs::metadata(local_path).await else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    let size_to_validate = if expected_size > 0 {
        expected_size
    } else {
        entry.size
    };
    size_to_validate == 0 || metadata.len() == size_to_validate
}

struct DownloadResult {
    path: PathBuf,
    size: u64,
    already_present: bool,
}

async fn download_instance(
    client: &Client,
    host: &str,
    http_port: u16,
    destination: &Path,
    instance: &TimelapseInstance,
    state: &Arc<AppState>,
    app: &AppHandle,
) -> anyhow::Result<DownloadResult> {
    let filename = local_filename(instance);
    let target = available_target(destination, &filename, instance.video_file_size).await?;
    if target.already_present {
        return Ok(DownloadResult {
            path: target.path,
            size: instance.video_file_size,
            already_present: true,
        });
    }

    let candidates = download_candidates(host, http_port, instance)?;
    let mut errors = Vec::new();
    for url in candidates {
        match download_url(
            client,
            &url,
            &target.path,
            instance.video_file_size,
            state,
            app,
        )
        .await
        {
            Ok(size) => {
                return Ok(DownloadResult {
                    path: target.path,
                    size,
                    already_present: false,
                })
            }
            Err(error) => errors.push(format!("{url}: {error}")),
        }
    }

    anyhow::bail!(
        "none of the download URLs returned by the U1 could be downloaded: {}",
        errors.join(" | ")
    )
}

struct TargetPath {
    path: PathBuf,
    already_present: bool,
}

async fn available_target(
    destination: &Path,
    filename: &str,
    expected_size: u64,
) -> anyhow::Result<TargetPath> {
    let preferred = destination.join(filename);
    if let Ok(metadata) = tokio::fs::metadata(&preferred).await {
        if expected_size > 0 && metadata.len() == expected_size {
            return Ok(TargetPath {
                path: preferred,
                already_present: true,
            });
        }
    } else {
        return Ok(TargetPath {
            path: preferred,
            already_present: false,
        });
    }

    let stem = preferred
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("timelapse");
    let extension = preferred
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4");
    for suffix in 2..10_000 {
        let candidate = destination.join(format!("{stem} ({suffix}).{extension}"));
        if !tokio::fs::try_exists(&candidate).await? {
            return Ok(TargetPath {
                path: candidate,
                already_present: false,
            });
        }
    }
    anyhow::bail!("could not create an available filename in the destination")
}

async fn download_url(
    client: &Client,
    url: &Url,
    target: &Path,
    expected_size: u64,
    state: &Arc<AppState>,
    app: &AppHandle,
) -> anyhow::Result<u64> {
    let response = client.get(url.clone()).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("HTTP {}", response.status());
    }
    let reported_size = response.content_length().unwrap_or(expected_size);
    let temporary = target.with_extension(format!(
        "{}.part",
        target
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("mp4")
    ));
    if tokio::fs::try_exists(&temporary).await? {
        tokio::fs::remove_file(&temporary).await?;
    }
    let mut file = tokio::fs::File::create(&temporary).await?;
    let mut stream = response.bytes_stream();
    let mut received = 0_u64;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        received += chunk.len() as u64;
        if let Some(progress) = received
            .saturating_mul(100)
            .checked_div(reported_size)
            .map(|value| value.min(99) as u8)
        {
            set_status(state, app, |status| status.progress_percent = progress).await;
        }
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    if let Err(error) = validate_download_size(expected_size, reported_size, received) {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }
    tokio::fs::rename(&temporary, target).await?;
    Ok(received)
}

fn validate_download_size(
    expected_size: u64,
    reported_size: u64,
    received: u64,
) -> anyhow::Result<()> {
    if expected_size > 0 && received != expected_size {
        anyhow::bail!("invalid size: expected {expected_size}, received {received}");
    }
    if reported_size > 0 && received != reported_size {
        anyhow::bail!("incomplete download: expected {reported_size}, received {received}");
    }
    Ok(())
}

fn download_candidates(
    host: &str,
    http_port: u16,
    instance: &TimelapseInstance,
) -> anyhow::Result<Vec<Url>> {
    let authority = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let base = Url::parse(&format!("http://{authority}:{http_port}/"))?;
    let mut candidates = Vec::<Url>::new();
    let suffix = instance.video_local_url_suffix.trim();

    if !suffix.is_empty() {
        if let Ok(url) = Url::parse(suffix) {
            ensure_local_url(&url, host)?;
            candidates.push(url);
        } else if suffix.starts_with(':') {
            let url = Url::parse(&format!("http://{authority}{suffix}"))?;
            ensure_local_url(&url, host)?;
            candidates.push(url);
        } else {
            candidates.push(base.join(suffix.trim_start_matches('/'))?);
            if http_port != 8080 {
                let fallback = Url::parse(&format!("http://{authority}:8080/"))?
                    .join(suffix.trim_start_matches('/'))?;
                candidates.push(fallback);
            }
        }
    }

    let remote_path = instance.video_path.replace('\\', "/");
    let camera_relative = remote_path
        .split("/camera/")
        .nth(1)
        .or_else(|| remote_path.strip_prefix("camera/"))
        .unwrap_or_else(|| {
            remote_path
                .rsplit('/')
                .next()
                .unwrap_or(remote_path.as_str())
        });
    if !camera_relative.is_empty() {
        let safe_path = camera_relative
            .split('/')
            .filter(|part| !part.is_empty() && *part != "." && *part != "..")
            .collect::<Vec<_>>()
            .join("/");
        let encoded = protocol::encode_url_path(&safe_path);
        candidates.push(base.join(&format!("server/files/camera/{encoded}"))?);
    }

    candidates.dedup_by(|left, right| left.as_str() == right.as_str());
    if candidates.is_empty() {
        anyhow::bail!("the timelapse does not contain a video URL or path");
    }
    Ok(candidates)
}

fn ensure_local_url(url: &Url, expected_host: &str) -> anyhow::Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        anyhow::bail!("the printer returned an unsupported download protocol");
    }
    let returned_host = url.host_str().unwrap_or_default();
    if !returned_host.eq_ignore_ascii_case(expected_host.trim_matches(['[', ']'])) {
        anyhow::bail!("the printer returned a URL for a different host");
    }
    Ok(())
}

fn local_filename(instance: &TimelapseInstance) -> String {
    let timestamp = if instance.unix_timestamp_s > 0 {
        Utc.timestamp_opt(instance.unix_timestamp_s, 0)
            .single()
            .unwrap_or_else(Utc::now)
    } else {
        Utc::now()
    };
    let raw_name = if instance.gcode_name.trim().is_empty() {
        "timelapse"
    } else {
        instance.gcode_name.trim()
    };
    let stem = sanitize_filename(raw_name);
    let extension = video_extension(instance);
    format!(
        "{}_{}.{}",
        timestamp.format("%Y-%m-%d_%H-%M-%S"),
        stem,
        extension
    )
}

fn video_extension(instance: &TimelapseInstance) -> String {
    let path = if instance.video_path.is_empty() {
        instance.video_local_url_suffix.as_str()
    } else {
        instance.video_path.as_str()
    };
    let extension = Path::new(path.split('?').next().unwrap_or(path))
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "mp4" | "mpeg" | "mpg" | "mov") {
        extension
    } else {
        "mp4".into()
    }
}

fn sanitize_filename(value: &str) -> String {
    let mut sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_control() || r#"<>:"/\|?*"#.contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect();
    sanitized = sanitized.trim().trim_end_matches(['.', ' ']).to_string();
    if sanitized.is_empty() {
        sanitized = "timelapse".into();
    }
    if sanitized.len() > 100 {
        sanitized.truncate(100);
    }
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3", "LPT4",
    ];
    if reserved
        .iter()
        .any(|name| sanitized.eq_ignore_ascii_case(name))
    {
        sanitized.insert(0, '_');
    }
    sanitized
}

async fn save_thumbnail(
    client: &Client,
    host: &str,
    http_port: u16,
    video_path: &Path,
    instance: &TimelapseInstance,
) -> anyhow::Result<()> {
    let bytes = if instance.thumbnail_base64.trim().is_empty() {
        if instance.thumbnail_path.trim().is_empty() {
            return Ok(());
        }
        let remote_path = instance.thumbnail_path.replace('\\', "/");
        let camera_relative = remote_path
            .split("/camera/")
            .nth(1)
            .or_else(|| remote_path.strip_prefix("camera/"))
            .unwrap_or(remote_path.as_str());
        let safe_path = camera_relative
            .split('/')
            .filter(|part| !part.is_empty() && *part != "." && *part != "..")
            .collect::<Vec<_>>()
            .join("/");
        let encoded = protocol::encode_url_path(&safe_path);
        let authority = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        client
            .get(format!(
                "http://{authority}:{http_port}/server/files/camera/{encoded}"
            ))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec()
    } else {
        let encoded = instance
            .thumbnail_base64
            .split_once(',')
            .map(|(_, data)| data)
            .unwrap_or(instance.thumbnail_base64.as_str());
        STANDARD.decode(encoded.trim())?
    };
    let thumbnail_path = video_path.with_extension("jpg");
    tokio::fs::write(thumbnail_path, bytes).await?;
    Ok(())
}

async fn update_counts(
    state: &Arc<AppState>,
    app: &AppHandle,
    summary: &SyncSummary,
    completed: usize,
    total: usize,
    current_file: Option<String>,
) {
    let stopping = state.stop_requested.load(Ordering::Acquire);
    set_status(state, app, |status| {
        status.phase = if stopping {
            "stopping".into()
        } else {
            "downloading".into()
        };
        status.downloaded = summary.downloaded;
        status.skipped = summary.skipped;
        status.failed = summary.failed;
        status.current_file = current_file;
        status.progress_percent = ((completed.saturating_mul(100) / total).min(99)) as u8;
    })
    .await;
}

async fn finish_with_error(
    state: &Arc<AppState>,
    app: &AppHandle,
    config: &AppConfig,
    message: String,
) {
    let now = Utc::now();
    set_status(state, app, |status| {
        status.phase = "error".into();
        status.running = false;
        status.current_file = None;
        status.last_error = Some(message);
        status.last_sync = Some(now);
        status.next_sync = config.next_sync_from(now);
    })
    .await;
}

async fn set_status<F>(state: &Arc<AppState>, app: &AppHandle, update: F)
where
    F: FnOnce(&mut SyncStatus),
{
    let snapshot = {
        let mut status = state.status.write().await;
        update(&mut status);
        status.clone()
    };
    let _ = app.emit("sync-status", snapshot);
}

#[cfg(test)]
fn fixture_contract() -> serde_json::Value {
    serde_json::from_str(include_str!("../tests/fixtures/timelapse_page.json"))
        .expect("timelapse fixture must contain valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance() -> TimelapseInstance {
        serde_json::from_value(fixture_contract()["instances"][0].clone()).unwrap()
    }

    #[test]
    fn builds_safe_camera_download_candidates() {
        let candidates = download_candidates("192.168.1.55", 7125, &instance()).unwrap();
        assert_eq!(
            candidates[0].as_str(),
            "http://192.168.1.55:7125/server/files/camera/cube.mp4"
        );
    }

    #[test]
    fn rejects_download_redirected_to_another_host() {
        let mut item = instance();
        item.video_local_url_suffix = "http://example.com/video.mp4".into();
        assert!(download_candidates("192.168.1.55", 7125, &item).is_err());
    }

    #[test]
    fn keeps_spaces_percent_encoded_in_download_urls() {
        let mut item = instance();
        item.video_path = "camera/Goku Hair Left.mp4".into();
        item.video_local_url_suffix = "/server/files/camera/Goku%20Hair%20Left.mp4".into();
        let candidates = download_candidates("192.168.1.55", 7125, &item).unwrap();
        assert!(candidates
            .iter()
            .all(|candidate| !candidate.as_str().contains('+')));
        assert!(candidates
            .iter()
            .any(|candidate| candidate.as_str().contains("Goku%20Hair%20Left.mp4")));
    }

    #[test]
    fn sanitizes_windows_file_names() {
        assert_eq!(sanitize_filename("cube:final?.gcode"), "cube_final_.gcode");
        assert_eq!(sanitize_filename("CON"), "_CON");
    }

    #[tokio::test]
    async fn keeps_existing_equal_file_and_renames_conflicts() {
        let directory = tempfile::tempdir().unwrap();
        let existing = directory.path().join("video.mp4");
        tokio::fs::write(&existing, [1_u8, 2, 3]).await.unwrap();

        let equal = available_target(directory.path(), "video.mp4", 3)
            .await
            .unwrap();
        assert!(equal.already_present);
        assert_eq!(equal.path, existing);

        let conflict = available_target(directory.path(), "video.mp4", 8)
            .await
            .unwrap();
        assert!(!conflict.already_present);
        assert_eq!(
            conflict.path.file_name().and_then(|value| value.to_str()),
            Some("video (2).mp4")
        );
    }

    #[test]
    fn rejects_interrupted_or_truncated_downloads() {
        assert!(validate_download_size(1_000, 1_000, 640).is_err());
        assert!(validate_download_size(0, 1_000, 640).is_err());
        assert!(validate_download_size(1_000, 1_000, 1_000).is_ok());
    }

    #[test]
    fn stop_signal_is_observed_before_the_next_file() {
        let signal = AtomicBool::new(false);
        assert!(!should_stop_before_next_file(&signal));
        signal.store(true, Ordering::Release);
        assert!(should_stop_before_next_file(&signal));
    }

    #[tokio::test]
    async fn deleted_or_changed_local_copy_is_downloaded_again() {
        let directory = tempfile::tempdir().unwrap();
        let local_path = directory.path().join("video.mp4");
        tokio::fs::write(&local_path, [1_u8, 2, 3]).await.unwrap();
        let entry = HistoryEntry {
            id: "history-1".into(),
            remote_key: "remote-1".into(),
            printer_sn: "U1".into(),
            gcode_name: "video".into(),
            remote_path: "camera/video.mp4".into(),
            local_path: local_path.to_string_lossy().to_string(),
            size: 3,
            completed_at: Utc::now(),
            result: HistoryResult::Downloaded,
            remote_deleted: false,
            message: String::new(),
        };

        assert!(local_copy_is_valid(&entry, directory.path(), 3).await);
        tokio::fs::remove_file(&local_path).await.unwrap();
        assert!(!local_copy_is_valid(&entry, directory.path(), 3).await);

        tokio::fs::write(&local_path, [1_u8, 2]).await.unwrap();
        assert!(!local_copy_is_valid(&entry, directory.path(), 3).await);
    }
}
