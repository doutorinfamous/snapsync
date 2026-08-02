use crate::models::{AppConfig, HistoryEntry, SyncStatus};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tokio::sync::{Mutex, RwLock};

pub struct AppState {
    pub data_dir: PathBuf,
    pub config: RwLock<AppConfig>,
    pub status: RwLock<SyncStatus>,
    pub history: RwLock<Vec<HistoryEntry>>,
    pub sync_lock: Mutex<()>,
    pub stop_requested: AtomicBool,
}

impl AppState {
    pub fn load(data_dir: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(&data_dir)?;
        let config: AppConfig = read_json(&data_dir.join("config.json")).unwrap_or_default();
        let history: Vec<HistoryEntry> =
            read_json(&data_dir.join("history.json")).unwrap_or_default();

        Ok(Self {
            data_dir,
            config: RwLock::new(config),
            status: RwLock::new(SyncStatus::default()),
            history: RwLock::new(history),
            sync_lock: Mutex::new(()),
            stop_requested: AtomicBool::new(false),
        })
    }

    pub async fn persist_config(&self) -> anyhow::Result<()> {
        let config = self.config.read().await.clone();
        write_json_atomic(&self.data_dir.join("config.json"), &config).await
    }

    pub async fn persist_history(&self) -> anyhow::Result<()> {
        let mut history = self.history.write().await;
        if history.len() > 500 {
            let remove_count = history.len() - 500;
            history.drain(0..remove_count);
        }
        write_json_atomic(&self.data_dir.join("history.json"), &*history).await
    }

    pub async fn add_history(&self, entry: HistoryEntry) -> anyhow::Result<()> {
        self.history.write().await.push(entry);
        self.persist_history().await
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let data = fs::read(path).ok()?;
    serde_json::from_slice(&data).ok()
}

async fn write_json_atomic<T: Serialize + ?Sized>(path: &Path, value: &T) -> anyhow::Result<()> {
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    tokio::fs::write(&temporary, bytes).await?;
    if tokio::fs::try_exists(path).await? {
        tokio::fs::remove_file(path).await?;
    }
    tokio::fs::rename(temporary, path).await?;
    Ok(())
}
