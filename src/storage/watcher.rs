use anyhow::{Result, Context};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use super::hasher::{hash_file, get_file_size};
use super::Database;

#[derive(Debug, Clone)]
pub struct SaveEvent {
    pub game_name: String,
    pub emulator: String,
    pub file_path: PathBuf,
    pub file_hash: String,
    pub file_size: u64,
}

pub struct SaveWatcher {
    watcher: Option<RecommendedWatcher>,
    save_dir: PathBuf,
    database: Arc<Database>,
    file_hashes: Arc<Mutex<HashMap<PathBuf, String>>>,
    sender: mpsc::Sender<SaveEvent>,
}

impl SaveWatcher {
    pub fn new(
        save_dir: PathBuf,
        database: Arc<Database>,
    ) -> Result<(Self, mpsc::Receiver<SaveEvent>)> {
        let (sender, receiver) = mpsc::channel(100);
        
        let watcher = SaveWatcher {
            watcher: None,
            save_dir,
            database,
            file_hashes: Arc::new(Mutex::new(HashMap::new())),
            sender,
        };
        
        Ok((watcher, receiver))
    }
    
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting save watcher for: {:?}", self.save_dir);
        
        // Check if directory exists
        if !self.save_dir.exists() {
            warn!("Save directory does not exist: {:?}", self.save_dir);
            return Ok(());
        }
        
        // Create file watcher
        let (tx, mut rx) = mpsc::channel(100);
        let file_hashes = self.file_hashes.clone();
        let sender = self.sender.clone();
        let save_dir = self.save_dir.clone();
        
        // Spawn handler for file events
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let Err(e) = Self::handle_event(
                    event,
                    &file_hashes,
                    &sender,
                    &save_dir,
                ).await {
                    error!("Error handling file event: {}", e);
                }
            }
        });
        
        // Create notify watcher
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        let _ = tx.blocking_send(event);
                    }
                    Err(e) => error!("Watch error: {:?}", e),
                }
            },
            Config::default(),
        )?;
        
        // Start watching directory
        watcher.watch(&self.save_dir, RecursiveMode::Recursive)?;
        self.watcher = Some(watcher);
        
        // Initial scan of existing files
        self.scan_existing_saves().await?;
        
        Ok(())
    }
    
    pub fn stop(&mut self) {
        if let Some(mut watcher) = self.watcher.take() {
            let _ = watcher.unwatch(&self.save_dir);
            info!("Stopped watching: {:?}", self.save_dir);
        }
    }
    
    async fn handle_event(
        event: Event,
        file_hashes: &Arc<Mutex<HashMap<PathBuf, String>>>,
        sender: &mpsc::Sender<SaveEvent>,
        save_dir: &Path,
    ) -> Result<()> {
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                for path in event.paths {
                    // Check if it's a save file (memory card or save state)
                    if Self::is_save_file(&path) {
                        debug!("Save file changed: {:?}", path);
                        
                        // Calculate file hash
                        let hash = match hash_file(&path) {
                            Ok(h) => h,
                            Err(e) => {
                                warn!("Failed to hash file {:?}: {}", path, e);
                                continue;
                            }
                        };
                        
                        // Check if file actually changed
                        let mut hashes = file_hashes.lock().await;
                        if let Some(old_hash) = hashes.get(&path) {
                            if old_hash == &hash {
                                debug!("File unchanged (same hash): {:?}", path);
                                continue;
                            }
                        }
                        
                        // Update hash
                        hashes.insert(path.clone(), hash.clone());
                        
                        // Get file size
                        let file_size = get_file_size(&path).unwrap_or(0);
                        
                        // Determine game name from file path
                        let game_name = Self::extract_game_name(&path, save_dir);
                        
                        // Send save event
                        let event = SaveEvent {
                            game_name,
                            emulator: "PCSX2".to_string(),
                            file_path: path.clone(),
                            file_hash: hash,
                            file_size,
                        };
                        
                        info!("Detected save: {} ({} bytes)", path.display(), file_size);
                        let _ = sender.send(event).await;
                    }
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    async fn scan_existing_saves(&mut self) -> Result<()> {
        debug!("Scanning existing saves in: {:?}", self.save_dir);
        
        let entries = std::fs::read_dir(&self.save_dir)?;
        let mut hashes = self.file_hashes.lock().await;
        
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            
            if Self::is_save_file(&path) {
                // Calculate and store initial hash
                if let Ok(hash) = hash_file(&path) {
                    hashes.insert(path.clone(), hash);
                    debug!("Indexed save file: {:?}", path);
                }
            }
        }
        
        info!("Indexed {} save files", hashes.len());
        Ok(())
    }
    
    fn is_save_file(path: &Path) -> bool {
        if let Some(extension) = path.extension() {
            let ext = extension.to_string_lossy().to_lowercase();
            // PCSX2 memory cards (.ps2) and save states (.p2s)
            matches!(ext.as_str(), "ps2" | "p2s" | "mcd" | "mcr")
        } else {
            false
        }
    }
    
    fn extract_game_name(path: &Path, save_dir: &Path) -> String {
        // Try to extract game name from file name or directory structure
        if let Some(file_name) = path.file_stem() {
            let name = file_name.to_string_lossy();
            // Remove common prefixes like "Mcd001" or similar
            if name.starts_with("Mcd") || name.starts_with("Memory") {
                "Unknown Game".to_string()
            } else {
                name.to_string()
            }
        } else {
            "Unknown Game".to_string()
        }
    }
}

/// Manager for handling save backup and versioning
pub struct SaveBackupManager {
    backup_dir: PathBuf,
    max_backups: usize,
}

impl SaveBackupManager {
    pub fn new(backup_dir: Option<PathBuf>) -> Result<Self> {
        let backup_dir = backup_dir.unwrap_or_else(|| {
            let dirs = directories::ProjectDirs::from("com", "retrosave", "retrosave")
                .expect("Failed to get project directories");
            let mut path = dirs.data_dir().to_path_buf();
            path.push("backups");
            path
        });
        
        // Ensure backup directory exists
        std::fs::create_dir_all(&backup_dir)?;
        
        Ok(Self {
            backup_dir,
            max_backups: 5,
        })
    }
    
    pub fn backup_save(&self, source: &Path, game_name: &str, version: u32) -> Result<PathBuf> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let file_name = format!("{}_{}_v{}.bak", game_name, timestamp, version);
        
        let mut backup_path = self.backup_dir.clone();
        backup_path.push(game_name);
        std::fs::create_dir_all(&backup_path)?;
        backup_path.push(file_name);
        
        std::fs::copy(source, &backup_path)
            .context("Failed to backup save file")?;
        
        info!("Backed up save to: {:?}", backup_path);
        Ok(backup_path)
    }
    
    pub fn cleanup_old_backups(&self, game_name: &str) -> Result<()> {
        let mut game_backup_dir = self.backup_dir.clone();
        game_backup_dir.push(game_name);
        
        if !game_backup_dir.exists() {
            return Ok(());
        }
        
        // Get all backup files
        let mut backups: Vec<_> = std::fs::read_dir(&game_backup_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.path().extension()
                    .map(|ext| ext == "bak")
                    .unwrap_or(false)
            })
            .collect();
        
        if backups.len() <= self.max_backups {
            return Ok(());
        }
        
        // Sort by modification time (oldest first)
        backups.sort_by_key(|entry| {
            entry.metadata()
                .and_then(|m| m.modified())
                .ok()
        });
        
        // Delete oldest backups
        let to_delete = backups.len() - self.max_backups;
        for entry in backups.iter().take(to_delete) {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                warn!("Failed to delete old backup: {}", e);
            } else {
                debug!("Deleted old backup: {:?}", entry.path());
            }
        }
        
        Ok(())
    }
}