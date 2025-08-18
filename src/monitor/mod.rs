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

#[derive(Debug, Clone)]
pub enum MonitorEvent {
    EmulatorStarted(String),
    EmulatorStopped(String),
    GameDetected(String),
    SaveDetected(String, String), // game_name, file_path
}

#[derive(Debug, Clone)]
pub enum MonitorCommand {
    TriggerManualSave,
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
    mut cmd_receiver: mpsc::Receiver<MonitorCommand>,
) -> Result<()> {
    info!("Process monitoring started with save detection");
    
    let mut interval = time::interval(Duration::from_secs(5));
    let mut tracked_emulators = HashSet::new();
    let mut save_watcher: Option<SaveWatcher> = None;
    let mut save_receiver: Option<mpsc::Receiver<SaveEvent>> = None;
    let backup_manager = SaveBackupManager::new(None)?;
    
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
                        if let Some(_watcher) = &save_watcher {
                            // TODO: Implement force save in watcher
                            info!("Checking all save files for changes...");
                        } else {
                            warn!("No active save watcher - no emulator running");
                        }
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
                                if let Err(e) = backup_manager.backup_save(
                                    &save_event.file_path,
                                    &game.name,
                                    save.version as u32,
                                ) {
                                    warn!("Failed to backup save: {}", e);
                                }
                                
                                // Clean up old saves (keep last 5)
                                if let Err(e) = database.cleanup_old_saves(game.id, 5).await {
                                    warn!("Failed to cleanup old saves: {}", e);
                                }
                                
                                // Clean up old backups
                                if let Err(e) = backup_manager.cleanup_old_backups(&game.name) {
                                    warn!("Failed to cleanup old backups: {}", e);
                                }
                                
                                // Send event
                                let _ = sender.send(MonitorEvent::SaveDetected(
                                    game.name,
                                    save_event.file_path.to_string_lossy().to_string(),
                                )).await;
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
            };
            
            // Check if this is a newly detected emulator
            if !tracked_emulators.contains(emulator_name) {
                tracked_emulators.insert(emulator_name.to_string());
                info!("{} started", emulator_name);
                let _ = sender.send(MonitorEvent::EmulatorStarted(emulator_name.to_string())).await;
                
                // Start save watching for PCSX2
                if emulator_name == "PCSX2" {
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
                
                // Try to detect the game after a short delay
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            
            match &emulator {
                process::EmulatorProcess::PCSX2 { pid, exe_path } => {
                    debug!("PCSX2 running - PID: {}, Path: {}", pid, exe_path);
                    
                    // Try to get the actual game name
                    if let Some(game_name) = process::get_pcsx2_game_name(*pid) {
                        let _ = sender.send(MonitorEvent::GameDetected(game_name)).await;
                    } else {
                        let _ = sender.send(MonitorEvent::GameDetected("Unknown Game".to_string())).await;
                    }
                }
            }
        } else {
            // Check if any tracked emulator has stopped
            if !tracked_emulators.is_empty() {
                // Stop save watcher
                if let Some(mut watcher) = save_watcher.take() {
                    watcher.stop();
                    info!("Stopped save watcher");
                }
                save_receiver = None;
                
                for emulator in tracked_emulators.drain() {
                    info!("{} stopped", emulator);
                    let _ = sender.send(MonitorEvent::EmulatorStopped(emulator)).await;
                }
            }
        }
    }
}