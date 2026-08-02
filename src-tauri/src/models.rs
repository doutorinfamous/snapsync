use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleUnit {
    #[default]
    Hours,
    Days,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PrinterConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub http_port: u16,
    pub sn: String,
    pub machine_type: String,
    pub paired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub version: u8,
    pub printer: Option<PrinterConfig>,
    pub destination: String,
    pub schedule_enabled: bool,
    pub interval_value: u64,
    pub interval_unit: ScheduleUnit,
    pub download_thumbnails: bool,
    pub autostart: bool,
    pub run_in_background: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            printer: None,
            destination: String::new(),
            schedule_enabled: true,
            interval_value: 12,
            interval_unit: ScheduleUnit::Hours,
            download_thumbnails: true,
            autostart: false,
            run_in_background: true,
        }
    }
}

impl AppConfig {
    pub fn schedule_duration(&self) -> chrono::Duration {
        let value = self.interval_value.max(1) as i64;
        match self.interval_unit {
            ScheduleUnit::Hours => chrono::Duration::hours(value),
            ScheduleUnit::Days => chrono::Duration::days(value),
        }
    }

    pub fn next_sync_from(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.schedule_enabled
            .then(|| now + self.schedule_duration())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPrinter {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub sn: String,
    pub machine_type: String,
    pub device_name: String,
    pub link_mode: String,
    pub region: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DirectConnectRequest {
    pub host: String,
    pub name: Option<String>,
    pub http_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TimelapseInstance {
    pub date_index: String,
    pub gcode_name: String,
    pub gcode_path: String,
    pub thumbnail_path: String,
    pub timelapse_dir: String,
    pub video_path: String,
    pub video_duration: String,
    pub generate_date: String,
    pub thumbnail_base64: String,
    pub video_local_url_suffix: String,
    pub video_file_size: u64,
    pub unix_timestamp_s: i64,
}

impl TimelapseInstance {
    pub fn remote_key(&self, printer_sn: &str) -> String {
        format!(
            "{}|{}|{}|{}",
            printer_sn, self.video_path, self.video_file_size, self.unix_timestamp_s
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryResult {
    Downloaded,
    AlreadyPresent,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub remote_key: String,
    pub printer_sn: String,
    pub gcode_name: String,
    pub remote_path: String,
    pub local_path: String,
    pub size: u64,
    pub completed_at: DateTime<Utc>,
    pub result: HistoryResult,
    pub remote_deleted: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub phase: String,
    pub running: bool,
    pub last_sync: Option<DateTime<Utc>>,
    pub next_sync: Option<DateTime<Utc>>,
    pub current_file: Option<String>,
    pub progress_percent: u8,
    pub downloaded: u64,
    pub skipped: u64,
    pub failed: u64,
    pub last_error: Option<String>,
}

impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            phase: "idle".into(),
            running: false,
            last_sync: None,
            next_sync: None,
            current_file: None,
            progress_percent: 0,
            downloaded: 0,
            skipped: 0,
            failed: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSnapshot {
    pub config: AppConfig,
    pub status: SyncStatus,
    pub history: Vec<HistoryEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncSummary {
    pub downloaded: u64,
    pub skipped: u64,
    pub failed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_converts_hours_and_days() {
        let mut config = AppConfig {
            interval_value: 6,
            interval_unit: ScheduleUnit::Hours,
            ..AppConfig::default()
        };
        assert_eq!(config.schedule_duration(), chrono::Duration::hours(6));

        config.interval_value = 3;
        config.interval_unit = ScheduleUnit::Days;
        assert_eq!(config.schedule_duration(), chrono::Duration::days(3));
    }

    #[test]
    fn disabled_schedule_has_no_next_execution() {
        let config = AppConfig {
            schedule_enabled: false,
            ..AppConfig::default()
        };
        assert_eq!(config.next_sync_from(Utc::now()), None);
    }

    #[test]
    fn old_minute_config_migrates_to_safe_hour_defaults() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "version": 1,
            "destination": "C:\\Backup",
            "interval_minutes": 15,
            "download_thumbnails": true,
            "autostart": false,
            "run_in_background": true
        }))
        .unwrap();
        assert!(config.schedule_enabled);
        assert_eq!(config.interval_value, 12);
        assert_eq!(config.interval_unit, ScheduleUnit::Hours);
        assert_eq!(config.destination, "C:\\Backup");
    }
}
