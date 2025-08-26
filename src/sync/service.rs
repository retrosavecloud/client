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
use super::{AuthManager, SyncApi, EncryptionManager, WebSocketClient, WsMessage};

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
    AuthFailed(String), // Authentication failed with reason
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
    encryption: Arc<RwLock<EncryptionManager>>,
    websocket: Arc<RwLock<Option<Arc<WebSocketClient>>>>,
    status: Arc<RwLock<SyncStatus>>,
    upload_queue: Arc<RwLock<VecDeque<UploadTask>>>,
    game_cache: Arc<RwLock<HashMap<String, Uuid>>>,
    conflict_strategy: ConflictResolutionStrategy,
    device_id: String,
    device_name: String,
}

#[derive(Debug, Clone, Copy)]
pub enum ConflictResolutionStrategy {
    NewerWins,      // Default: newer timestamp wins
    LocalFirst,     // Always prefer local changes
    CloudFirst,     // Always prefer cloud changes
    Manual,         // Ask user (not implemented yet)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
        data_dir: Option<std::path::PathBuf>,
    ) -> Self {
        let api = Arc::new(SyncApi::new(api_base_url.clone(), auth_manager.clone()));
        let encryption = Arc::new(RwLock::new(EncryptionManager::new(data_dir)));
        
        // Generate device ID and name
        let device_id = Uuid::new_v4().to_string();
        let device_name = gethostname::gethostname()
            .to_string_lossy()
            .to_string();
        
        Self {
            auth_manager,
            api,
            database,
            encryption,
            websocket: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(SyncStatus {
                is_syncing: false,
                last_sync: None,
                pending_uploads: 0,
                pending_downloads: 0,
                total_synced: 0,
            })),
            upload_queue: Arc::new(RwLock::new(VecDeque::new())),
            game_cache: Arc::new(RwLock::new(HashMap::new())),
            conflict_strategy: ConflictResolutionStrategy::NewerWins,
            device_id,
            device_name,
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
        
        // Restore upload queue from database
        if let Err(e) = self.restore_upload_queue().await {
            warn!("Failed to restore upload queue: {}", e);
        }
        
        // Initialize WebSocket if authenticated
        let auth_state = self.auth_manager.get_state().await;
        if auth_state.is_authenticated {
            if let Some(tokens) = auth_state.tokens {
                self.clone().init_websocket(tokens.access_token);
            }
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
                    drop(queue); // Release lock before persisting
                    
                    // Persist queue to database
                    if let Err(e) = self.persist_upload_queue().await {
                        warn!("Failed to persist upload queue: {}", e);
                    }
                    
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
                        
                        // Initialize WebSocket
                        let auth_state = self.auth_manager.get_state().await;
                        if let Some(tokens) = auth_state.tokens {
                            self.clone().init_websocket(tokens.access_token);
                        }
                        
                        // Trigger sync on login
                        let sync_service = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = sync_service.perform_sync().await {
                                error!("Post-login sync failed: {}", e);
                            }
                        });
                    } else {
                        // Disconnect WebSocket on logout
                        self.disconnect_websocket().await;
                    }
                }
                
                SyncEvent::AuthFailed(reason) => {
                    warn!("Authentication failed: {}", reason);
                    // Auth manager already handles logout/cleanup
                    // Just disconnect WebSocket
                    self.disconnect_websocket().await;
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
        
        // Notify sync started via WebSocket
        self.notify_sync_started().await;
        
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
            
            // Get or register game with cloud (returns UUID)
            let cloud_game_id = self.get_or_register_game(&task.game_name, &task.emulator).await?;
            
            // Also ensure we have a local game record
            let local_game = self.database
                .get_or_create_game(&task.game_name, &task.emulator)
                .await?;
            
            // Read file data
            let data = tokio::fs::read(&task.file_path).await
                .context("Failed to read save file")?;
            
            // Optionally encrypt before compression
            let processed_data = {
                let encryption = self.encryption.read().await;
                if encryption.is_enabled() {
                    debug!("Encrypting save before upload");
                    let encrypted_save = encryption.encrypt_save(&data)
                        .context("Failed to encrypt save")?;
                    serde_json::to_vec(&encrypted_save)
                        .context("Failed to serialize encrypted save")?
                } else {
                    data
                }
            };
            
            // Compress data
            let compressed_data = zstd::encode_all(processed_data.as_slice(), 3)
                .context("Failed to compress save")?;
            
            // Calculate hash of compressed data
            let mut hasher = Sha256::new();
            hasher.update(&compressed_data);
            let hash = format!("{:x}", hasher.finalize());
            
            // Request upload URL with file path in metadata
            let upload_response = self.api
                .request_upload_url_with_metadata(
                    cloud_game_id, 
                    &hash, 
                    compressed_data.len() as i64, 
                    task.timestamp,
                    Some(serde_json::json!({
                        "file_path": task.file_path.clone(),
                        "game_name": task.game_name.clone(),
                        "emulator": task.emulator.clone(),
                    }))
                )
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
            
            // Persist updated queue (or clear if empty)
            let queue = self.upload_queue.read().await;
            if queue.is_empty() {
                if let Err(e) = self.clear_persisted_queue().await {
                    warn!("Failed to clear persisted queue: {}", e);
                }
            } else {
                drop(queue); // Release lock before persisting
                if let Err(e) = self.persist_upload_queue().await {
                    warn!("Failed to persist updated queue: {}", e);
                }
            }
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
        
        info!("Found {} saves in cloud", saves_response.total);
        
        // Track downloads
        let mut downloaded = 0;
        let mut pending_downloads = saves_response.saves.len();
        
        // Update pending downloads count
        {
            let mut status = self.status.write().await;
            status.pending_downloads = pending_downloads;
        }
        
        // Process each cloud save
        for cloud_save in saves_response.saves {
            // For now, skip saves we can't map to local games
            // In a full implementation, we'd maintain a UUID->i64 mapping
            // or store cloud game IDs in local database
            
            // Extract game info from metadata if available
            let game_name = cloud_save.metadata
                .as_ref()
                .and_then(|m| m.get("game_name"))
                .and_then(|n| n.as_str());
            let emulator = cloud_save.metadata
                .as_ref()
                .and_then(|m| m.get("emulator"))
                .and_then(|e| e.as_str());
            
            // Skip if we don't have game info
            let (game_name, emulator) = match (game_name, emulator) {
                (Some(name), Some(emu)) => (name, emu),
                _ => {
                    debug!("Skipping save {} - no game metadata", cloud_save.id);
                    continue;
                }
            };
            
            // Get or create local game
            let local_game = self.database
                .get_or_create_game(game_name, emulator)
                .await?;
            
            let local_saves = self.database
                .get_saves_for_game(local_game.id, Some(10))
                .await?;
            
            // Check if we already have this save by hash
            let have_locally = local_saves.iter().any(|s| s.file_hash == cloud_save.file_hash);
            
            // Download based on conflict resolution strategy
            let should_download = if have_locally {
                match self.conflict_strategy {
                    ConflictResolutionStrategy::CloudFirst => true,
                    ConflictResolutionStrategy::LocalFirst => false,
                    ConflictResolutionStrategy::NewerWins => {
                        // Check if cloud version is newer
                        local_saves.iter()
                            .filter(|s| s.file_hash == cloud_save.file_hash)
                            .all(|s| cloud_save.created_at > s.timestamp)
                    },
                    ConflictResolutionStrategy::Manual => {
                        warn!("Manual conflict resolution not implemented, using NewerWins");
                        local_saves.iter()
                            .filter(|s| s.file_hash == cloud_save.file_hash)
                            .all(|s| cloud_save.created_at > s.timestamp)
                    }
                }
            } else {
                true // No local save, download it
            };
            
            if should_download {
                debug!("Downloading save {} from {}", cloud_save.file_hash, cloud_save.created_at);
                
                // Get download URL from API
                if let Some(download_url) = cloud_save.download_url {
                    // Download the save data
                    match self.api.download_save_data(&download_url).await {
                        Ok(compressed_data) => {
                            // Decompress the data
                            match zstd::decode_all(compressed_data.as_slice()) {
                                Ok(decompressed_data) => {
                                    // Check if data is encrypted and decrypt if needed
                                    let final_data = {
                                        // Try to parse as encrypted save
                                        if let Ok(encrypted_save) = serde_json::from_slice::<super::encryption::EncryptedSave>(&decompressed_data) {
                                            let encryption = self.encryption.read().await;
                                            if encryption.is_enabled() {
                                                debug!("Decrypting downloaded save");
                                                match encryption.decrypt_save(&encrypted_save) {
                                                    Ok(decrypted) => decrypted,
                                                    Err(e) => {
                                                        warn!("Failed to decrypt save: {}", e);
                                                        decompressed_data // Use as-is if decryption fails
                                                    }
                                                }
                                            } else {
                                                warn!("Encrypted save received but encryption not enabled");
                                                decompressed_data
                                            }
                                        } else {
                                            // Not encrypted, use as-is
                                            decompressed_data
                                        }
                                    };
                                    
                                    // Extract file path from metadata
                                    let file_path = cloud_save.metadata
                                        .as_ref()
                                        .and_then(|m| m.get("file_path"))
                                        .and_then(|p| p.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| format!("cloud_save_{}", cloud_save.id));
                                    
                                    // Record in database
                                    self.database.record_save(
                                        local_game.id,
                                        &file_path,
                                        &cloud_save.file_hash,
                                        cloud_save.file_size,
                                        Some(&format!("cloud_{}", cloud_save.id)),
                                    ).await?;
                                    
                                    // Try to restore the actual file if we have a valid path
                                    if let Some(metadata) = &cloud_save.metadata {
                                        if let Some(original_path) = metadata.get("file_path").and_then(|p| p.as_str()) {
                                            let path = std::path::PathBuf::from(original_path);
                                            if let Some(parent) = path.parent() {
                                                tokio::fs::create_dir_all(parent).await.ok();
                                            }
                                            if let Err(e) = tokio::fs::write(&path, &final_data).await {
                                                warn!("Failed to write save file {}: {}", original_path, e);
                                            } else {
                                                info!("Downloaded and restored save: {}", original_path);
                                            }
                                        }
                                    }
                                    
                                    downloaded += 1;
                                },
                                Err(e) => warn!("Failed to decompress save data: {}", e),
                            }
                        },
                        Err(e) => warn!("Failed to download save {}: {}", cloud_save.id, e),
                    }
                } else {
                    debug!("No download URL for save {}", cloud_save.id);
                }
            } else {
                debug!("Skipping save {} - local version is up to date", cloud_save.file_hash);
            }
            
            pending_downloads -= 1;
            
            // Update status
            {
                let mut status = self.status.write().await;
                status.pending_downloads = pending_downloads;
            }
        }
        
        if downloaded > 0 {
            info!("Downloaded {} saves from cloud", downloaded);
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
    
    
    /// Set conflict resolution strategy
    pub fn set_conflict_strategy(&mut self, strategy: ConflictResolutionStrategy) {
        self.conflict_strategy = strategy;
        info!("Conflict resolution strategy set to: {:?}", strategy);
    }
    
    /// Get pending upload count
    pub async fn get_pending_uploads(&self) -> usize {
        self.upload_queue.read().await.len()
    }
    
    /// Clear upload queue
    pub async fn clear_upload_queue(&self) {
        let mut queue = self.upload_queue.write().await;
        let count = queue.len();
        queue.clear();
        info!("Cleared {} pending uploads from queue", count);
    }
    
    /// Retry failed uploads
    pub async fn retry_failed_uploads(&self) -> Result<()> {
        let auth_state = self.auth_manager.get_state().await;
        if !auth_state.is_authenticated {
            return Err(anyhow::anyhow!("Not authenticated"));
        }
        
        info!("Retrying failed uploads...");
        self.process_upload_queue().await
    }
    
    /// Save upload queue to database for persistence
    pub async fn persist_upload_queue(&self) -> Result<()> {
        let queue = self.upload_queue.read().await;
        if queue.is_empty() {
            debug!("No uploads to persist");
            return Ok(());
        }
        
        // Serialize queue to JSON
        let queue_data = serde_json::to_string(&queue.iter().collect::<Vec<_>>())
            .context("Failed to serialize upload queue")?;
        
        // Store in database settings or a dedicated table
        self.database
            .save_setting("upload_queue", &queue_data)
            .await
            .context("Failed to persist upload queue")?;
        
        info!("Persisted {} uploads to database", queue.len());
        Ok(())
    }
    
    /// Restore upload queue from database
    pub async fn restore_upload_queue(&self) -> Result<()> {
        // Load from database
        let queue_data = match self.database.get_setting("upload_queue").await? {
            Some(data) => data,
            None => {
                debug!("No persisted upload queue found");
                return Ok(());
            }
        };
        
        // Deserialize queue
        let tasks: Vec<UploadTask> = serde_json::from_str(&queue_data)
            .context("Failed to deserialize upload queue")?;
        
        if tasks.is_empty() {
            return Ok(());
        }
        
        // Restore to queue
        let mut queue = self.upload_queue.write().await;
        for task in tasks {
            queue.push_back(task);
        }
        
        info!("Restored {} uploads from database", queue.len());
        
        // Update status
        let mut status = self.status.write().await;
        status.pending_uploads = queue.len();
        
        Ok(())
    }
    
    /// Clear persisted queue from database
    pub async fn clear_persisted_queue(&self) -> Result<()> {
        self.database
            .delete_setting("upload_queue")
            .await
            .context("Failed to clear persisted queue")?;
        
        debug!("Cleared persisted upload queue");
        Ok(())
    }
    
    /// Shutdown handler - persist any pending uploads
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down sync service");
        
        // Persist any pending uploads
        let queue = self.upload_queue.read().await;
        if !queue.is_empty() {
            drop(queue); // Release lock before persisting
            if let Err(e) = self.persist_upload_queue().await {
                error!("Failed to persist upload queue on shutdown: {}", e);
            } else {
                info!("Persisted upload queue on shutdown");
            }
        }
        
        Ok(())
    }
    
    /// Enable E2E encryption with password
    pub async fn enable_encryption(&self, password: &str) -> Result<()> {
        let mut encryption = self.encryption.write().await;
        encryption.init_with_password(password).await?;
        info!("E2E encryption enabled for sync");
        Ok(())
    }
    
    /// Disable E2E encryption
    pub async fn disable_encryption(&self) {
        let mut encryption = self.encryption.write().await;
        *encryption = EncryptionManager::new(None);
        info!("E2E encryption disabled");
    }
    
    /// Check if encryption is enabled
    pub async fn is_encryption_enabled(&self) -> bool {
        let encryption = self.encryption.read().await;
        encryption.is_enabled()
    }
    
    /// Change encryption password
    pub async fn change_encryption_password(&self, old_password: &str, new_password: &str) -> Result<()> {
        let mut encryption = self.encryption.write().await;
        encryption.change_password(old_password, new_password).await
    }
    
    /// Export encryption key for backup
    pub async fn export_encryption_key(&self) -> Result<String> {
        let encryption = self.encryption.read().await;
        encryption.export_key()
    }
    
    /// Import encryption key from backup
    pub async fn import_encryption_key(&self, key: &str) -> Result<()> {
        let mut encryption = self.encryption.write().await;
        encryption.import_key(key)
    }
    
    /// Initialize WebSocket connection
    fn init_websocket(self: Arc<Self>, token: String) {
        tokio::spawn(async move {
            let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<WsMessage>();
            
            let api_url = self.api.base_url.clone();
            let mut client = WebSocketClient::new(api_url, ws_tx);
            client.set_token(token).await;
            
            // Connect to server
            if let Err(e) = client.connect().await {
                warn!("Failed to connect WebSocket: {}", e);
                return;
            }
            
            let client = Arc::new(client);
            
            // Store client
            let mut ws = self.websocket.write().await;
            *ws = Some(client.clone());
            
            // Start listening for messages
            let client_clone = client.clone();
            tokio::spawn(async move {
                client_clone.start_listening().await;
            });
            
            // Handle incoming WebSocket messages
            let sync_service = self.clone();
            tokio::spawn(async move {
                while let Some(msg) = ws_rx.recv().await {
                    sync_service.clone().handle_ws_message(msg).await;
                }
            });
            
            info!("WebSocket connected and listening");
        });
    }
    
    /// Disconnect WebSocket
    async fn disconnect_websocket(&self) {
        let mut ws = self.websocket.write().await;
        if let Some(client) = ws.take() {
            if let Err(e) = client.disconnect().await {
                warn!("Failed to disconnect WebSocket: {}", e);
            }
        }
    }
    
    /// Handle incoming WebSocket messages
    async fn handle_ws_message(self: Arc<Self>, msg: WsMessage) {
        match msg {
            WsMessage::RequestSync => {
                info!("Sync requested via WebSocket");
                let sync_service = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = sync_service.perform_sync().await {
                        error!("WebSocket-triggered sync failed: {}", e);
                    }
                });
            }
            WsMessage::SaveUploaded { game_name, .. } => {
                info!("Save uploaded notification for {}", game_name);
                // Could trigger a download check here
            }
            WsMessage::SyncCompleted { device_name, uploads, downloads, .. } => {
                info!("Device {} completed sync: {} uploads, {} downloads", 
                      device_name, uploads, downloads);
            }
            _ => {
                debug!("Received WebSocket message: {:?}", msg);
            }
        }
    }
    
    /// Notify sync started via WebSocket
    async fn notify_sync_started(&self) {
        let ws = self.websocket.read().await;
        if let Some(client) = ws.as_ref() {
            if let Err(e) = client.notify_sync_started(
                self.device_id.clone(),
                self.device_name.clone()
            ).await {
                debug!("Failed to notify sync started: {}", e);
            }
        }
    }
    
    /// Notify sync completed via WebSocket
    async fn notify_sync_completed(&self, uploads: usize, downloads: usize) {
        let ws = self.websocket.read().await;
        if let Some(client) = ws.as_ref() {
            if let Err(e) = client.notify_sync_completed(
                self.device_id.clone(),
                self.device_name.clone(),
                uploads,
                downloads
            ).await {
                debug!("Failed to notify sync completed: {}", e);
            }
        }
    }
    
    /// Notify save uploaded via WebSocket
    async fn notify_save_uploaded(&self, game_id: String, game_name: String, emulator: String, save_id: String) {
        let ws = self.websocket.read().await;
        if let Some(client) = ws.as_ref() {
            if let Err(e) = client.notify_save_uploaded(
                game_id,
                game_name,
                emulator,
                save_id
            ).await {
                debug!("Failed to notify save uploaded: {}", e);
            }
        }
    }

}