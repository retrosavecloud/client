use anyhow::{Result, Context};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration};
use tracing::{info, error, debug, warn};
use chrono::Utc;
use uuid::Uuid;
use std::collections::{HashMap, VecDeque};
use sha2::{Sha256, Digest};

use crate::storage::database::Database;
use super::{AuthManager, SyncApi};

#[derive(Debug, Clone)]
pub enum SyncEvent {
    SaveDetected {
        game_name: String,
        emulator: String,
        file_path: String,
        file_hash: String,
        file_size: i64,
    },
    SyncRequested,
    AuthChanged(bool),
}

#[derive(Debug, Clone)]
pub struct SyncStatus {
    pub is_syncing: bool,
    pub last_sync: Option<chrono::DateTime<Utc>>,
    pub pending_uploads: usize,
    pub pending_downloads: usize,
    pub total_synced: usize,
}

pub struct SyncService {
    auth_manager: Arc<AuthManager>,
    api: Arc<SyncApi>,
    database: Arc<Database>,
    status: Arc<RwLock<SyncStatus>>,
    upload_queue: Arc<RwLock<VecDeque<UploadTask>>>,
    game_cache: Arc<RwLock<HashMap<String, Uuid>>>,
}

#[derive(Debug, Clone)]
struct UploadTask {
    game_name: String,
    emulator: String,
    file_path: String,
    file_hash: String,
    file_size: i64,
    timestamp: chrono::DateTime<Utc>,
}

impl SyncService {
    pub fn new(
        auth_manager: Arc<AuthManager>,
        database: Arc<Database>,
        api_base_url: String,
    ) -> Self {
        let api = Arc::new(SyncApi::new(api_base_url, auth_manager.clone()));
        
        Self {
            auth_manager,
            api,
            database,
            status: Arc::new(RwLock::new(SyncStatus {
                is_syncing: false,
                last_sync: None,
                pending_uploads: 0,
                pending_downloads: 0,
                total_synced: 0,
            })),
            upload_queue: Arc::new(RwLock::new(VecDeque::new())),
            game_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start the sync service
    pub async fn start(
        self: Arc<Self>,
        mut event_rx: mpsc::UnboundedReceiver<SyncEvent>,
    ) -> Result<()> {
        info!("Starting sync service");

        // Initialize auth
        if let Err(e) = self.auth_manager.init().await {
            warn!("Failed to initialize auth: {}", e);
        }

        // Spawn periodic sync task
        let sync_service = self.clone();
        tokio::spawn(async move {
            let mut sync_interval = interval(Duration::from_secs(300)); // Sync every 5 minutes
            
            loop {
                sync_interval.tick().await;
                
                let auth_state = sync_service.auth_manager.get_state().await;
                if auth_state.is_authenticated {
                    if let Err(e) = sync_service.perform_sync().await {
                        error!("Periodic sync failed: {}", e);
                    }
                }
            }
        });

        // Handle events
        while let Some(event) = event_rx.recv().await {
            match event {
                SyncEvent::SaveDetected { game_name, emulator, file_path, file_hash, file_size } => {
                    debug!("Save detected: {} for {}", game_name, emulator);
                    
                    // Add to upload queue
                    let task = UploadTask {
                        game_name,
                        emulator,
                        file_path,
                        file_hash,
                        file_size,
                        timestamp: Utc::now(),
                    };
                    
                    let mut queue = self.upload_queue.write().await;
                    queue.push_back(task);
                    
                    let mut status = self.status.write().await;
                    status.pending_uploads = queue.len();
                    
                    // Trigger sync if authenticated
                    let auth_state = self.auth_manager.get_state().await;
                    if auth_state.is_authenticated {
                        let sync_service = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = sync_service.perform_sync().await {
                                error!("Sync failed: {}", e);
                            }
                        });
                    }
                }
                
                SyncEvent::SyncRequested => {
                    info!("Manual sync requested");
                    let auth_state = self.auth_manager.get_state().await;
                    if auth_state.is_authenticated {
                        let sync_service = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = sync_service.perform_sync().await {
                                error!("Manual sync failed: {}", e);
                            }
                        });
                    } else {
                        warn!("Sync requested but not authenticated");
                    }
                }
                
                SyncEvent::AuthChanged(is_authenticated) => {
                    info!("Auth state changed: authenticated={}", is_authenticated);
                    if is_authenticated {
                        // Clear game cache to refresh from server
                        self.game_cache.write().await.clear();
                        
                        // Trigger sync on login
                        let sync_service = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = sync_service.perform_sync().await {
                                error!("Post-login sync failed: {}", e);
                            }
                        });
                    }
                }
            }
        }

        Ok(())
    }

    /// Perform synchronization
    async fn perform_sync(&self) -> Result<()> {
        // Check if already syncing
        {
            let mut status = self.status.write().await;
            if status.is_syncing {
                debug!("Sync already in progress");
                return Ok(());
            }
            status.is_syncing = true;
        }

        info!("Starting sync");
        
        // Upload pending saves
        let upload_result = self.process_upload_queue().await;
        
        // Download new saves
        let download_result = self.download_new_saves().await;
        
        // Update status
        {
            let mut status = self.status.write().await;
            status.is_syncing = false;
            status.last_sync = Some(Utc::now());
            
            if upload_result.is_ok() && download_result.is_ok() {
                info!("Sync completed successfully");
            } else {
                warn!("Sync completed with errors");
            }
        }

        Ok(())
    }

    /// Process upload queue
    async fn process_upload_queue(&self) -> Result<()> {
        let mut processed = 0;
        
        loop {
            let task = {
                let mut queue = self.upload_queue.write().await;
                queue.pop_front()
            };
            
            let Some(task) = task else {
                break;
            };
            
            debug!("Processing upload: {} for {}", task.game_name, task.emulator);
            
            // Get or register game
            let game_id = self.get_or_register_game(&task.game_name, &task.emulator).await?;
            
            // Read file data
            let data = tokio::fs::read(&task.file_path).await
                .context("Failed to read save file")?;
            
            // Compress data
            let compressed_data = zstd::encode_all(data.as_slice(), 3)
                .context("Failed to compress save")?;
            
            // Calculate hash of compressed data
            let mut hasher = Sha256::new();
            hasher.update(&compressed_data);
            let hash = format!("{:x}", hasher.finalize());
            
            // Request upload URL
            let upload_response = self.api
                .request_upload_url(game_id, &hash, compressed_data.len() as i64, task.timestamp)
                .await?;
            
            // Upload data
            self.api
                .upload_save_data(&upload_response.upload_url, compressed_data)
                .await?;
            
            processed += 1;
            info!("Uploaded save for {}", task.game_name);
            
            // Update status
            {
                let mut status = self.status.write().await;
                let queue = self.upload_queue.read().await;
                status.pending_uploads = queue.len();
                status.total_synced += 1;
            }
        }
        
        if processed > 0 {
            info!("Uploaded {} saves", processed);
        }
        
        Ok(())
    }

    /// Download new saves from cloud
    async fn download_new_saves(&self) -> Result<()> {
        // Get list of saves from server
        let saves_response = self.api.list_saves(None, 1, 100).await?;
        
        if saves_response.saves.is_empty() {
            debug!("No saves to download");
            return Ok(());
        }
        
        // TODO: Compare with local saves and download newer ones
        // For now, we'll just log what's available
        info!("Found {} saves in cloud", saves_response.total);
        
        // Update status
        {
            let mut status = self.status.write().await;
            status.pending_downloads = 0;
        }
        
        Ok(())
    }

    /// Get sync status
    pub async fn get_status(&self) -> SyncStatus {
        self.status.read().await.clone()
    }
    
    /// Trigger manual sync
    pub async fn trigger_sync(&self) -> Result<()> {
        let auth_state = self.auth_manager.get_state().await;
        if !auth_state.is_authenticated {
            return Err(anyhow::anyhow!("Not authenticated"));
        }
        
        self.perform_sync().await
    }
    
    /// Get or register a game
    async fn get_or_register_game(&self, name: &str, emulator: &str) -> Result<Uuid> {
        let cache_key = format!("{}:{}", name, emulator);
        
        // Check cache
        {
            let cache = self.game_cache.read().await;
            if let Some(game_id) = cache.get(&cache_key) {
                return Ok(*game_id);
            }
        }
        
        // Register with API
        let game = self.api.register_game(name, emulator).await?;
        
        // Update cache
        {
            let mut cache = self.game_cache.write().await;
            cache.insert(cache_key, game.id);
        }
        
        Ok(game.id)
    }

}