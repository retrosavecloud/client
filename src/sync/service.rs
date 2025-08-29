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
use crate::storage::save_types::{SaveType, MemoryCardFormat};
use crate::storage::ps2_memory_card::PS2MemoryCard;
use super::{AuthManager, SyncApi, EncryptionManager, WebSocketClient, WsMessage};
use super::api::SaveMetadata;

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
    notification_service: Option<Arc<crate::ui::notifications::NotificationManager>>,
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
            notification_service: None,
        }
    }
    
    /// Set the notification service
    pub fn with_notification_service(mut self, service: Arc<crate::ui::notifications::NotificationManager>) -> Self {
        self.notification_service = Some(service);
        self
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
            let mut sync_interval = interval(Duration::from_secs(1800)); // Sync every 30 minutes
            
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
            let upload_response = match self.api
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
                .await {
                    Ok(response) => response,
                    Err(e) => {
                        // Check if this is a limit exceeded error
                        let error_str = e.to_string();
                        if error_str.contains("402") || error_str.contains("limit") || error_str.contains("exceeded") {
                            warn!("Cloud sync limit exceeded for {}: {}", task.game_name, e);
                            
                            // Show notification about limit
                            if let Some(ref notif) = self.notification_service {
                                notif.show_warning(
                                    "Cloud Sync Limit Reached",
                                    &format!("Save for {} was saved locally but couldn't sync to cloud. Upgrade your plan for more cloud storage.", task.game_name)
                                );
                            }
                            
                            // Keep the task in queue for later retry
                            continue;
                        }
                        
                        // For other errors, propagate them
                        return Err(e);
                    }
                };
            
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
        
        if saves_response.items.is_empty() {
            debug!("No saves to download");
            return Ok(());
        }
        
        info!("Found {} saves in cloud", saves_response.total);
        
        // DEBUG: Log all saves we received
        for save in &saves_response.items {
            let game_name = save.metadata.as_ref()
                .and_then(|m| m.get("game_name"))
                .and_then(|g| g.as_str())
                .unwrap_or("unknown");
            let version = save.version.unwrap_or(-1) as i64;
            let has_metadata = save.metadata.as_ref()
                .and_then(|m| m.get("memory_card_metadata"))
                .is_some();
            info!("  Cloud save: {} v{} hash={} has_metadata={} timestamp={}", 
                game_name, version, &save.file_hash[0..8], has_metadata, save.client_timestamp);
        }
        
        // Group saves by file path to avoid downloading multiple versions of the same file
        // IMPORTANT: Group by game name + file name to handle old saves without full paths
        let mut saves_by_path: std::collections::HashMap<String, Vec<SaveMetadata>> = std::collections::HashMap::new();
        
        for save in saves_response.items {
            // Try to determine the logical grouping key
            // CRITICAL: We must group ALL saves for the same game together!
            let group_key = if let Some(metadata) = &save.metadata {
                // Use game name as the primary grouping key to ensure all saves for a game compete
                if let Some(game_name) = metadata.get("game_name").and_then(|g| g.as_str()) {
                    // Group by game name - this ensures old and new saves compete
                    format!("game:{}", game_name)
                } else if let Some(file_path) = metadata.get("file_path").and_then(|p| p.as_str()) {
                    // Fall back to file path if no game name
                    format!("path:{}", file_path)
                } else {
                    // Last resort - unique key
                    format!("unknown_{}", save.id)
                }
            } else {
                // No metadata at all - skip these old saves
                warn!("Skipping save {} with no metadata at all", save.id);
                continue;
            };
            
            info!("Grouping save under key '{}': hash={} timestamp={}", 
                group_key, &save.file_hash[0..8], save.client_timestamp);
            saves_by_path.entry(group_key).or_default().push(save);
        }
        
        // For each file path, only keep the best save (prefer ones with metadata)
        let mut newest_saves = Vec::new();
        info!("Grouped saves into {} unique file paths", saves_by_path.len());
        for (path, mut saves) in saves_by_path {
            if saves.len() > 1 {
                // Sort by:
                // 1. Prefer saves WITH memory_card_metadata
                // 2. Then by timestamp descending (newest first)
                // Log before sorting
                for (i, save) in saves.iter().enumerate() {
                    let has_meta = save.metadata.as_ref()
                        .and_then(|m| m.get("memory_card_metadata"))
                        .is_some();
                    let version = save.version.unwrap_or(-1) as i64;
                    info!("  Before sort #{}: v{} has_metadata={} timestamp={} hash={}", 
                        i, version, has_meta, save.client_timestamp, &save.file_hash[0..8]);
                }
                
                saves.sort_by(|a, b| {
                    let a_has_metadata = a.metadata.as_ref()
                        .and_then(|m| m.get("memory_card_metadata"))
                        .is_some();
                    let b_has_metadata = b.metadata.as_ref()
                        .and_then(|m| m.get("memory_card_metadata"))
                        .is_some();
                    
                    match (a_has_metadata, b_has_metadata) {
                        (true, false) => std::cmp::Ordering::Less, // a is better
                        (false, true) => std::cmp::Ordering::Greater, // b is better
                        _ => b.client_timestamp.cmp(&a.client_timestamp) // both same, use newest
                    }
                });
                
                let selected = &saves[0];
                let has_metadata = selected.metadata.as_ref()
                    .and_then(|m| m.get("memory_card_metadata"))
                    .is_some();
                
                info!("DEBUG: Found {} cloud saves for {}, selected v{} from {} (has_metadata: {}, hash={})", 
                    saves.len(), path, 
                    selected.version.unwrap_or(-1),
                    selected.client_timestamp, 
                    has_metadata,
                    &selected.file_hash[0..8]);
            }
            if let Some(newest) = saves.into_iter().next() {
                debug!("Adding save to download: {} from {}", newest.file_hash, newest.client_timestamp);
                newest_saves.push(newest);
            }
        }
        info!("Will check {} deduplicated saves for download", newest_saves.len());
        
        // Track downloads
        let mut downloaded = 0;
        let mut pending_downloads = newest_saves.len();
        
        // Update pending downloads count
        {
            let mut status = self.status.write().await;
            status.pending_downloads = pending_downloads;
        }
        
        // Process each cloud save (now deduplicated by path)
        for cloud_save in newest_saves {
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
            
            // For memory cards, also check if the actual file hash matches
            let file_hash_matches = if let Some(metadata) = &cloud_save.metadata {
                if let Some(file_path) = metadata.get("file_path").and_then(|p| p.as_str()) {
                    if let Ok(data) = tokio::fs::read(file_path).await {
                        use sha2::{Sha256, Digest};
                        let mut hasher = Sha256::new();
                        hasher.update(&data);
                        let hash = format!("{:x}", hasher.finalize());
                        let matches = hash == cloud_save.file_hash;
                        if !matches {
                            info!("Memory card hash mismatch - local: {}, cloud: {}", hash, cloud_save.file_hash);
                        }
                        matches
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };
            
            // Check if the actual file exists on disk and has the specific game save
            let (file_exists, needs_restore) = if let Some(metadata) = &cloud_save.metadata {
                if let Some(file_path) = metadata.get("file_path").and_then(|p| p.as_str()) {
                    if tokio::fs::metadata(file_path).await.is_ok() {
                        // File exists, check if it has the actual game saves
                        let emulator = metadata.get("emulator")
                            .and_then(|e| e.as_str())
                            .unwrap_or("");
                        
                        let save_type = SaveType::detect(&std::path::PathBuf::from(file_path), emulator);
                        
                        if let SaveType::MemoryCard { format, .. } = save_type {
                            // Read file and check content
                            if let Ok(data) = tokio::fs::read(file_path).await {
                                match format {
                                    MemoryCardFormat::PS2 => {
                                        // Parse PS2 memory card to check for game saves
                                        if let Some(card) = PS2MemoryCard::new(data.clone()) {
                                            let game_name = metadata.get("game_name")
                                                .and_then(|g| g.as_str())
                                                .unwrap_or("Harry Potter");
                                            
                                            // Generate metadata for safety checks
                                            let local_metadata = card.generate_metadata(game_name.to_string());
                                            
                                            // Check if we have the specific game save
                                            let has_game_save = card.has_game_saves(game_name) || 
                                                               card.has_harry_potter_save();
                                            
                                            // SAFETY CHECK: If we have OTHER games, be careful
                                            if local_metadata.games_contained.len() > 1 {
                                                // Parse cloud metadata if available
                                                let cloud_metadata = metadata.get("memory_card_metadata")
                                                    .and_then(|m| serde_json::from_value::<retrosave_shared::MemoryCardMetadata>(m.clone()).ok());
                                                
                                                if let Some(cloud_meta) = cloud_metadata {
                                                    // Analyze conflicts
                                                    use crate::sync::conflict_resolution::{ConflictAnalyzer, ResolutionStrategy};
                                                    
                                                    let local_hash = crate::storage::hasher::hash_bytes(&data);
                                                    let cloud_hash = cloud_save.file_hash.clone();
                                                    let local_time = chrono::Utc::now(); // Should get actual file time
                                                    let cloud_time = cloud_save.client_timestamp;
                                                    
                                                    let conflicts = ConflictAnalyzer::analyze_memory_card_conflicts(
                                                        &local_metadata,
                                                        &cloud_meta,
                                                        &local_hash,
                                                        &cloud_hash,
                                                        local_time,
                                                        cloud_time,
                                                    );
                                                    
                                                    if !conflicts.is_empty() {
                                                        warn!("Found {} conflicts in memory card", conflicts.len());
                                                        for conflict in &conflicts {
                                                            warn!("  - {} ({}): {:?}", 
                                                                conflict.game_name, 
                                                                conflict.game_id,
                                                                conflict.conflict_type
                                                            );
                                                        }
                                                        
                                                        // For now, use smart strategy (can be made interactive later)
                                                        let resolution = ConflictAnalyzer::resolve_conflicts(
                                                            &conflicts,
                                                            ResolutionStrategy::Smart,
                                                        );
                                                        
                                                        info!("Conflict resolution: {} local, {} cloud, {} merged",
                                                            resolution.games_kept_local.len(),
                                                            resolution.games_kept_cloud.len(),
                                                            resolution.games_merged.len()
                                                        );
                                                        
                                                        // If we're keeping any local games, don't overwrite
                                                        if !resolution.games_kept_local.is_empty() {
                                                            warn!("Keeping local games, skipping download to prevent data loss");
                                                            (true, false) // Don't download
                                                        } else {
                                                            (true, true) // Safe to download
                                                        }
                                                    } else {
                                                        // No conflicts, safe to proceed
                                                        if !has_game_save {
                                                            info!("Memory card exists but doesn't have {} saves", game_name);
                                                            (true, true) // Exists but needs restore
                                                        } else {
                                                            debug!("Memory card has {} saves", game_name);
                                                            (true, false) // Has saves, don't need restore
                                                        }
                                                    }
                                                } else {
                                                    // No cloud metadata (old save format)
                                                    // Check if we should allow download for migration
                                                    let allow_migration = {
                                                        // Allow if memory card is mostly empty
                                                        let empty_games = local_metadata.games_contained.iter()
                                                            .filter(|g| g.save_count == 0)
                                                            .count();
                                                        let total_games = local_metadata.games_contained.len();
                                                        
                                                        // If most games are empty, allow migration
                                                        if empty_games > total_games / 2 {
                                                            info!("Memory card is mostly empty ({}/{} empty), allowing migration from old cloud save", 
                                                                empty_games, total_games);
                                                            true
                                                        } else if !has_game_save && local_metadata.games_contained.len() <= 5 {
                                                            // If we don't have this specific game and there are few games
                                                            info!("Memory card missing {} and has few games ({}), allowing migration",
                                                                game_name, local_metadata.games_contained.len());
                                                            true
                                                        } else {
                                                            warn!("No cloud metadata available, being cautious with {} games. Consider clearing memory card for migration.", 
                                                                local_metadata.games_contained.len());
                                                            false
                                                        }
                                                    };
                                                    
                                                    if allow_migration && !has_game_save {
                                                        (true, true) // Allow restore for migration
                                                    } else {
                                                        (true, false) // Don't download without metadata
                                                    }
                                                }
                                            } else {
                                                // Single game or empty, safe to proceed
                                                if !has_game_save {
                                                    info!("Memory card exists but doesn't have {} saves", game_name);
                                                    (true, true) // Exists but needs restore
                                                } else {
                                                    debug!("Memory card has {} saves", game_name);
                                                    (true, false) // Has saves, don't need restore
                                                }
                                            }
                                        } else {
                                            info!("Invalid PS2 memory card format");
                                            (true, true) // Invalid, needs restore
                                        }
                                    },
                                    _ => {
                                        // For other formats, use simple empty check
                                        let is_empty = format.is_empty(&data);
                                        if is_empty {
                                            info!("Memory card exists but is empty: {}", file_path);
                                        }
                                        (true, is_empty)
                                    }
                                }
                            } else {
                                (true, false)
                            }
                        } else {
                            (true, false)
                        }
                    } else {
                        (false, true) // Doesn't exist, needs restore
                    }
                } else {
                    (false, true)
                }
            } else {
                (false, true)
            };
            
            // Download based on conflict resolution strategy
            // For memory cards: respect the safety checks from above
            let should_download = if !file_hash_matches && needs_restore {
                // Only download if safety checks passed
                info!("Will download save - hash mismatch and safety checks passed, path: {:?}", 
                    cloud_save.metadata.as_ref()
                        .and_then(|m| m.get("file_path"))
                        .and_then(|p| p.as_str()));
                true
            } else if !file_exists || needs_restore {
                info!("Will download save - exists: {}, needs_restore: {}, path: {:?}", 
                    file_exists, needs_restore,
                    cloud_save.metadata.as_ref()
                        .and_then(|m| m.get("file_path"))
                        .and_then(|p| p.as_str()));
                true
            } else if have_locally {
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
                            info!("Downloaded {} compressed bytes from S3", compressed_data.len());
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
                                            
                                            // Debug: Log data size and first bytes
                                            info!("Writing {} bytes to {}", final_data.len(), original_path);
                                            if final_data.len() > 0 {
                                                debug!("First 16 bytes: {:?}", &final_data[..16.min(final_data.len())]);
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