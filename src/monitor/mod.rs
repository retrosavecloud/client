pub mod process;

use anyhow::Result;
use std::time::Duration;
use std::collections::HashSet;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::time;
use tokio::sync::mpsc;
use tracing::{info, debug, warn, error};

use crate::storage::{Database, SaveWatcher, SaveEvent, SaveBackupManager};
use crate::sync::SyncEvent;
use crate::emulators::Emulator;

#[derive(Debug, Clone)]
pub enum MonitorEvent {
    EmulatorStarted(String),
    EmulatorStopped(String),
    GameDetected(String),
    SaveDetected {
        game_name: String,
        emulator: String,
        file_path: String,
    },
    ManualSaveResult(SaveResult),
}

#[derive(Debug, Clone)]
pub enum MonitorCommand {
    TriggerManualSave,
}

#[derive(Debug, Clone)]
pub enum SaveResult {
    Success { game_name: String, file_count: usize },
    NoChanges,
    Failed(String),
}

pub async fn start_monitoring() -> Result<()> {
    let db = Arc::new(Database::new(None).await?);
    let (sender, _receiver) = mpsc::channel(100);
    start_monitoring_with_db(sender, db).await
}

pub async fn start_monitoring_with_sender(sender: mpsc::Sender<MonitorEvent>) -> Result<()> {
    let db = Arc::new(Database::new(None).await?);
    start_monitoring_with_db(sender, db).await
}

pub async fn start_monitoring_with_db(
    sender: mpsc::Sender<MonitorEvent>,
    database: Arc<Database>,
) -> Result<()> {
    let (_cmd_sender, cmd_receiver) = mpsc::channel(10);
    start_monitoring_with_commands(sender, database, cmd_receiver).await
}

pub async fn start_monitoring_with_commands(
    sender: mpsc::Sender<MonitorEvent>,
    database: Arc<Database>,
    cmd_receiver: mpsc::Receiver<MonitorCommand>,
) -> Result<()> {
    start_monitoring_with_sync(sender, database, cmd_receiver, None).await
}

pub async fn start_monitoring_with_sync(
    sender: mpsc::Sender<MonitorEvent>,
    database: Arc<Database>,
    mut cmd_receiver: mpsc::Receiver<MonitorCommand>,
    sync_sender: Option<mpsc::UnboundedSender<SyncEvent>>,
) -> Result<()> {
    info!("Process monitoring started with save detection");
    
    let mut interval = time::interval(Duration::from_secs(5));
    let mut tracked_emulators = HashSet::new();
    let mut save_watcher: Option<SaveWatcher> = None;
    let mut save_receiver: Option<mpsc::Receiver<SaveEvent>> = None;
    let backup_manager = SaveBackupManager::new(None)?;
    let mut current_game_name: Option<String> = None;
    
    loop {
        tokio::select! {
            _ = interval.tick() => {
                // Regular monitoring tick
            }
            Some(cmd) = cmd_receiver.recv() => {
                // Handle commands
                match cmd {
                    MonitorCommand::TriggerManualSave => {
                        info!("Manual save triggered");
                        // Force save detection for all tracked saves
                        let result = if let Some(ref watcher) = save_watcher {
                            // Check for actual file changes
                            match watcher.check_for_changes().await {
                                Ok(changes) => {
                                    if changes > 0 {
                                        let game_name = current_game_name.clone()
                                            .unwrap_or_else(|| "Unknown Game".to_string());
                                        SaveResult::Success { 
                                            game_name,
                                            file_count: changes 
                                        }
                                    } else {
                                        SaveResult::NoChanges
                                    }
                                }
                                Err(e) => SaveResult::Failed(e.to_string())
                            }
                        } else {
                            SaveResult::Failed("No emulator running".to_string())
                        };
                        
                        // Send result back through event system
                        let _ = sender.send(MonitorEvent::ManualSaveResult(result)).await;
                    }
                }
                continue;
            }
        }
        
        // Check for save events
        if let Some(receiver) = &mut save_receiver {
            while let Ok(save_event) = receiver.try_recv() {
                info!("Save detected: {} - {}", save_event.game_name, save_event.file_path.display());
                
                // Record save in database
                match database.get_or_create_game(&save_event.game_name, &save_event.emulator).await {
                    Ok(game) => {
                        // Record the save
                        match database.record_save(
                            game.id,
                            &save_event.file_path.to_string_lossy(),
                            &save_event.file_hash,
                            save_event.file_size as i64,
                            None,
                        ).await {
                            Ok(save) => {
                                info!("Recorded save #{} for {}", save.version, game.name);
                                
                                // Backup the save
                                match backup_manager.backup_save(
                                    &save_event.file_path,
                                    &game.name,
                                    save.version as u32,
                                ) {
                                    Ok((_backup_path, stats)) => {
                                        if let Some(compression_stats) = stats {
                                            debug!(
                                                "Compressed backup: {} -> {} ({}% saved)",
                                                compression_stats.original_size,
                                                compression_stats.compressed_size,
                                                compression_stats.space_saved_percent() as u32
                                            );
                                        }
                                    }
                                    Err(e) => warn!("Failed to backup save: {}", e),
                                }
                                
                                // Clean up old saves (keep last 5)
                                if let Err(e) = database.cleanup_old_saves(game.id, 5).await {
                                    warn!("Failed to cleanup old saves: {}", e);
                                }
                                
                                // Clean up old backups
                                if let Err(e) = backup_manager.cleanup_old_backups(&game.name) {
                                    warn!("Failed to cleanup old backups: {}", e);
                                }
                                
                                // Send monitor event
                                let _ = sender.send(MonitorEvent::SaveDetected {
                                    game_name: game.name.clone(),
                                    emulator: save_event.emulator.clone(),
                                    file_path: save_event.file_path.to_string_lossy().to_string(),
                                }).await;
                                
                                // Send sync event if sync is enabled
                                if let Some(ref sync_tx) = sync_sender {
                                    let _ = sync_tx.send(SyncEvent::SaveDetected {
                                        game_name: game.name,
                                        emulator: save_event.emulator,
                                        file_path: save_event.file_path.to_string_lossy().to_string(),
                                        file_hash: save_event.file_hash,
                                        file_size: save_event.file_size as i64,
                                    });
                                }
                            }
                            Err(e) => error!("Failed to record save: {}", e),
                        }
                    }
                    Err(e) => error!("Failed to get/create game: {}", e),
                }
            }
        }
        
        // Check for running emulators
        if let Some(emulator) = process::detect_running_emulators() {
            let emulator_name = match &emulator {
                process::EmulatorProcess::PCSX2 { .. } => "PCSX2",
                process::EmulatorProcess::Dolphin { .. } => "Dolphin",
                process::EmulatorProcess::RPCS3 { .. } => "RPCS3",
                process::EmulatorProcess::Citra { .. } => "Citra",
            };
            
            // Check if this is a newly detected emulator
            if !tracked_emulators.contains(emulator_name) {
                tracked_emulators.insert(emulator_name.to_string());
                info!("{} started", emulator_name);
                let _ = sender.send(MonitorEvent::EmulatorStarted(emulator_name.to_string())).await;
                
                // Start save watching for the emulator
                match emulator_name {
                    "PCSX2" => {
                    if let Some(save_dir) = process::get_pcsx2_save_directory() {
                        let save_dir = PathBuf::from(save_dir);
                        match SaveWatcher::new(save_dir.clone(), database.clone()) {
                            Ok((mut watcher, receiver)) => {
                                if let Err(e) = watcher.start().await {
                                    warn!("Failed to start save watcher: {}", e);
                                } else {
                                    info!("Started save watcher for PCSX2");
                                    save_watcher = Some(watcher);
                                    save_receiver = Some(receiver);
                                }
                            }
                            Err(e) => warn!("Failed to create save watcher: {}", e),
                        }
                    } else {
                        warn!("Could not find PCSX2 save directory");
                    }
                    }
                    "Dolphin" => {
                        if let process::EmulatorProcess::Dolphin { .. } = &emulator {
                            let dolphin = crate::emulators::dolphin::Dolphin::new();
                            if let Some(save_dir) = dolphin.get_save_directory() {
                                let save_dir = PathBuf::from(save_dir);
                                match SaveWatcher::new(save_dir.clone(), database.clone()) {
                                    Ok((mut watcher, receiver)) => {
                                        if let Err(e) = watcher.start().await {
                                            warn!("Failed to start save watcher: {}", e);
                                        } else {
                                            info!("Started save watcher for Dolphin");
                                            save_watcher = Some(watcher);
                                            save_receiver = Some(receiver);
                                        }
                                    }
                                    Err(e) => warn!("Failed to create save watcher: {}", e),
                                }
                            } else {
                                warn!("Could not find Dolphin save directory");
                            }
                        }
                    }
                    "RPCS3" => {
                        if let process::EmulatorProcess::RPCS3 { .. } = &emulator {
                            let rpcs3 = crate::emulators::rpcs3::RPCS3::new();
                            if let Some(save_dir) = rpcs3.get_save_directory() {
                                let save_dir = PathBuf::from(save_dir);
                                match SaveWatcher::new(save_dir.clone(), database.clone()) {
                                    Ok((mut watcher, receiver)) => {
                                        if let Err(e) = watcher.start().await {
                                            warn!("Failed to start save watcher: {}", e);
                                        } else {
                                            info!("Started save watcher for RPCS3");
                                            save_watcher = Some(watcher);
                                            save_receiver = Some(receiver);
                                        }
                                    }
                                    Err(e) => warn!("Failed to create save watcher: {}", e),
                                }
                            } else {
                                warn!("Could not find RPCS3 save directory");
                            }
                        }
                    }
                    "Citra" => {
                        if let process::EmulatorProcess::Citra { .. } = &emulator {
                            let citra = crate::emulators::citra::Citra::new();
                            if let Some(save_dir) = citra.get_save_directory() {
                                let save_dir = PathBuf::from(save_dir);
                                match SaveWatcher::new(save_dir.clone(), database.clone()) {
                                    Ok((mut watcher, receiver)) => {
                                        if let Err(e) = watcher.start().await {
                                            warn!("Failed to start save watcher: {}", e);
                                        } else {
                                            info!("Started save watcher for Citra");
                                            save_watcher = Some(watcher);
                                            save_receiver = Some(receiver);
                                        }
                                    }
                                    Err(e) => warn!("Failed to create save watcher: {}", e),
                                }
                            } else {
                                warn!("Could not find Citra save directory");
                            }
                        }
                    }
                    _ => {}
                }
                
                // Try to detect the game after a short delay
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            
            match &emulator {
                process::EmulatorProcess::PCSX2 { pid, exe_path } => {
                    debug!("PCSX2 running - PID: {}, Path: {}", pid, exe_path);
                    
                    // Try to get the actual game name
                    if let Some(game_name) = process::get_pcsx2_game_name(*pid) {
                        current_game_name = Some(game_name.clone());
                        
                        // Update SaveWatcher with the current game name
                        if let Some(ref watcher) = save_watcher {
                            watcher.set_current_game(Some(game_name.clone())).await;
                        }
                        
                        let _ = sender.send(MonitorEvent::GameDetected(game_name)).await;
                    } else {
                        current_game_name = Some("Unknown Game".to_string());
                        
                        // Clear game name in SaveWatcher
                        if let Some(ref watcher) = save_watcher {
                            watcher.set_current_game(None).await;
                        }
                        
                        let _ = sender.send(MonitorEvent::GameDetected("Unknown Game".to_string())).await;
                    }
                }
                process::EmulatorProcess::Dolphin { pid, exe_path } => {
                    debug!("Dolphin running - PID: {}, Path: {}", pid, exe_path);
                    
                    // Try to get the actual game name
                    if let Some(game_name) = process::get_dolphin_game_name(*pid) {
                        current_game_name = Some(game_name.clone());
                        
                        // Update SaveWatcher with the current game name
                        if let Some(ref watcher) = save_watcher {
                            watcher.set_current_game(Some(game_name.clone())).await;
                        }
                        
                        let _ = sender.send(MonitorEvent::GameDetected(game_name)).await;
                    } else {
                        current_game_name = Some("Unknown GameCube/Wii Game".to_string());
                        
                        // Clear game name in SaveWatcher
                        if let Some(ref watcher) = save_watcher {
                            watcher.set_current_game(None).await;
                        }
                        
                        let _ = sender.send(MonitorEvent::GameDetected("Unknown GameCube/Wii Game".to_string())).await;
                    }
                }
                process::EmulatorProcess::RPCS3 { pid, exe_path } => {
                    debug!("RPCS3 running - PID: {}, Path: {}", pid, exe_path);
                    
                    // Try to get the actual game name
                    if let Some(game_name) = process::get_rpcs3_game_name(*pid) {
                        current_game_name = Some(game_name.clone());
                        
                        // Update SaveWatcher with the current game name
                        if let Some(ref watcher) = save_watcher {
                            watcher.set_current_game(Some(game_name.clone())).await;
                        }
                        
                        let _ = sender.send(MonitorEvent::GameDetected(game_name)).await;
                    } else {
                        current_game_name = Some("Unknown PS3 Game".to_string());
                        
                        // Clear game name in SaveWatcher
                        if let Some(ref watcher) = save_watcher {
                            watcher.set_current_game(None).await;
                        }
                        
                        let _ = sender.send(MonitorEvent::GameDetected("Unknown PS3 Game".to_string())).await;
                    }
                }
                process::EmulatorProcess::Citra { pid, exe_path } => {
                    debug!("Citra running - PID: {}, Path: {}", pid, exe_path);
                    
                    // Try to get the actual game name
                    if let Some(game_name) = process::get_citra_game_name(*pid) {
                        current_game_name = Some(game_name.clone());
                        
                        // Update SaveWatcher with the current game name
                        if let Some(ref watcher) = save_watcher {
                            watcher.set_current_game(Some(game_name.clone())).await;
                        }
                        
                        let _ = sender.send(MonitorEvent::GameDetected(game_name)).await;
                    } else {
                        current_game_name = Some("Unknown 3DS Game".to_string());
                        
                        // Clear game name in SaveWatcher
                        if let Some(ref watcher) = save_watcher {
                            watcher.set_current_game(None).await;
                        }
                        
                        let _ = sender.send(MonitorEvent::GameDetected("Unknown 3DS Game".to_string())).await;
                    }
                }
            }
        } else {
            // Check if any tracked emulator has stopped
            if !tracked_emulators.is_empty() {
                // Stop save watcher
                if let Some(mut watcher) = save_watcher.take() {
                    // Clear game name before stopping
                    watcher.set_current_game(None).await;
                    watcher.stop();
                    info!("Stopped save watcher");
                }
                save_receiver = None;
                
                for emulator in tracked_emulators.drain() {
                    info!("{} stopped", emulator);
                    let _ = sender.send(MonitorEvent::EmulatorStopped(emulator)).await;
                    current_game_name = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_monitor_event_creation() {
        let event = MonitorEvent::EmulatorStarted("PCSX2".to_string());
        match event {
            MonitorEvent::EmulatorStarted(name) => assert_eq!(name, "PCSX2"),
            _ => panic!("Wrong event type"),
        }

        let event = MonitorEvent::SaveDetected {
            game_name: "Final Fantasy X".to_string(),
            emulator: "PCSX2".to_string(),
            file_path: "/path/to/save".to_string(),
        };
        match event {
            MonitorEvent::SaveDetected { game_name, emulator, file_path } => {
                assert_eq!(game_name, "Final Fantasy X");
                assert_eq!(emulator, "PCSX2");
                assert_eq!(file_path, "/path/to/save");
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_save_result() {
        let result = SaveResult::Success {
            game_name: "Test Game".to_string(),
            file_count: 3,
        };
        match result {
            SaveResult::Success { game_name, file_count } => {
                assert_eq!(game_name, "Test Game");
                assert_eq!(file_count, 3);
            }
            _ => panic!("Wrong result type"),
        }

        let result = SaveResult::NoChanges;
        matches!(result, SaveResult::NoChanges);

        let result = SaveResult::Failed("Error message".to_string());
        match result {
            SaveResult::Failed(msg) => assert_eq!(msg, "Error message"),
            _ => panic!("Wrong result type"),
        }
    }

    #[tokio::test]
    async fn test_monitor_command() {
        let cmd = MonitorCommand::TriggerManualSave;
        matches!(cmd, MonitorCommand::TriggerManualSave);
    }

    #[tokio::test]
    async fn test_event_channel() {
        let (sender, mut receiver) = mpsc::channel::<MonitorEvent>(10);
        
        // Send events
        sender.send(MonitorEvent::EmulatorStarted("Test".to_string())).await.unwrap();
        sender.send(MonitorEvent::EmulatorStopped("Test".to_string())).await.unwrap();
        
        // Receive events
        let event1 = receiver.recv().await.unwrap();
        matches!(event1, MonitorEvent::EmulatorStarted(_));
        
        let event2 = receiver.recv().await.unwrap();
        matches!(event2, MonitorEvent::EmulatorStopped(_));
    }
}