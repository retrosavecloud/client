use anyhow::Result;
use tracing::{info, error, debug};
use tracing_subscriber;
use tokio::sync::mpsc;

mod monitor;
mod emulators;
mod ui;
mod storage;
mod hotkey;
mod sync;

use ui::{SystemTray, tray::TrayMessage, SettingsWindow, NotificationManager, AudioFeedback};
use storage::{Database, SettingsManager};
use hotkey::{HotkeyManager, HotkeyEvent};
use sync::{AuthManager, SyncService, SyncEvent};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("retrosave=debug")
        .init();

    info!("Starting Retrosave...");

    // Get data directory
    let data_dir = dirs::data_dir()
        .map(|d| d.join("retrosave"))
        .unwrap_or_else(|| std::path::PathBuf::from(".retrosave"));
    
    // Create data directory if it doesn't exist
    tokio::fs::create_dir_all(&data_dir).await?;

    // Initialize database
    let db_path = data_dir.join("retrosave.db");
    let db = Arc::new(Database::new(Some(db_path)).await?);
    info!("Database initialized");
    
    // Get database stats
    let (games, saves) = db.get_stats().await?;
    info!("Database stats: {} games, {} saves", games, saves);
    
    // Initialize settings manager and load settings
    let settings_manager = Arc::new(SettingsManager::new(db.clone()));
    let saved_settings = settings_manager.load_settings().await?;
    info!("Settings loaded from database");

    // Initialize system tray
    let (tray, mut tray_receiver) = SystemTray::new()?;
    info!("System tray initialized");

    // Initialize auth manager early so we can pass it to settings window
    let auth_manager = Arc::new(AuthManager::new(saved_settings.cloud_api_url.clone()));
    
    // Initialize auth manager to load tokens from keyring
    {
        let auth_manager_clone = auth_manager.clone();
        tokio::spawn(async move {
            if let Err(e) = auth_manager_clone.init().await {
                error!("Failed to initialize auth manager: {}", e);
            } else {
                info!("Auth manager initialized, checking stored tokens...");
            }
        });
    }
    
    // Create settings window with settings manager and auth manager
    let settings_window = Arc::new(SettingsWindow::with_auth_manager(
        saved_settings,
        settings_manager.clone(),
        auth_manager.clone()
    )?);

    // Create notification manager for desktop notifications
    let notif_manager = Arc::new(NotificationManager::new());
    
    // Create audio feedback for save events
    let audio_feedback = Arc::new(AudioFeedback::default());

    // Create channels for monitor communication
    let (monitor_sender, mut monitor_receiver) = mpsc::channel::<monitor::MonitorEvent>(100);
    let (cmd_sender, cmd_receiver) = mpsc::channel::<monitor::MonitorCommand>(10);
    
    // Create hotkey manager
    let (hotkey_sender, mut hotkey_receiver) = mpsc::channel::<HotkeyEvent>(100);
    let hotkey_manager = Arc::new(HotkeyManager::new(hotkey_sender)?);
    
    // Set up initial hotkey from settings
    let settings = settings_window.get_settings();
    if settings.hotkey_enabled {
        hotkey_manager.set_save_hotkey(settings.save_hotkey.clone())?;
    }
    
    // Start hotkey listener
    hotkey_manager.clone().start_listening();
    
    // Initialize cloud sync service
    let (sync_event_sender, sync_event_receiver) = mpsc::unbounded_channel::<SyncEvent>();
    let sync_service = Arc::new(SyncService::new(
        auth_manager.clone(),
        db.clone(),
        settings.cloud_api_url.clone(),
        Some(data_dir.clone()),
    ));
    
    // Start sync service if cloud sync is enabled
    if settings.cloud_sync_enabled {
        let sync_service_clone = sync_service.clone();
        tokio::spawn(async move {
            if let Err(e) = sync_service_clone.start(sync_event_receiver).await {
                error!("Sync service error: {}", e);
            }
        });
    }

    // Start process monitoring with database, command channel, and sync integration
    let db_clone = db.clone();
    let sync_sender_for_monitor = if settings.cloud_sync_enabled {
        Some(sync_event_sender.clone())
    } else {
        None
    };
    let monitor_handle = tokio::spawn(async move {
        if let Err(e) = monitor::start_monitoring_with_sync(
            monitor_sender, 
            db_clone, 
            cmd_receiver,
            sync_sender_for_monitor
        ).await {
            error!("Monitor error: {}", e);
        }
    });

    // Handle monitor events and update tray
    let cmd_sender_clone = cmd_sender.clone();
    let settings_window_clone = settings_window.clone();
    let notif_manager_clone = notif_manager.clone();
    let audio_feedback_clone = audio_feedback.clone();
    let cmd_sender_hotkey = cmd_sender.clone();
    let hotkey_manager_clone = hotkey_manager.clone();
    let sync_event_sender_clone = sync_event_sender.clone();
    let event_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(hotkey_event) = hotkey_receiver.recv() => {
                    match hotkey_event {
                        HotkeyEvent::SaveNow => {
                            info!("Hotkey triggered: Save Now");
                            // Don't show notification here, wait for the result
                            let _ = cmd_sender_hotkey.send(monitor::MonitorCommand::TriggerManualSave).await;
                        }
                    }
                }
                Some(event) = monitor_receiver.recv() => {
                    match event {
                        monitor::MonitorEvent::EmulatorStarted(name) => {
                            let msg = format!("{} detected", name);
                            tray.update_status(&msg);
                            
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_emulator_detected(&name);
                            }
                            
                            tray.show_notification("Emulator Detected", &msg);
                            let _ = tray.send_message(TrayMessage::EmulatorDetected(name)).await;
                        }
                        monitor::MonitorEvent::EmulatorStopped(name) => {
                            tray.update_status("Monitoring");
                            
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_emulator_stopped(&name);
                            }
                            
                            tray.show_notification("Emulator Stopped", &format!("{} has stopped", name));
                            let _ = tray.send_message(TrayMessage::EmulatorStopped).await;
                        }
                        monitor::MonitorEvent::GameDetected(name) => {
                            tray.update_status(&format!("Playing: {}", name));
                            
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_game_detected(&name);
                            }
                            
                            let _ = tray.send_message(TrayMessage::GameDetected(name)).await;
                        }
                        monitor::MonitorEvent::SaveDetected { game_name, emulator, file_path } => {
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_save_detected(&game_name);
                            }
                            
                            tray.show_notification("Save Detected", &format!("{} saved", game_name));
                            let _ = tray.send_message(TrayMessage::SaveDetected(format!("{}: {}", game_name, file_path))).await;
                            
                            // Send to sync service if cloud sync is enabled
                            let settings = settings_window_clone.get_settings();
                            if settings.cloud_sync_enabled {
                                // Calculate file hash and size
                                if let Ok(data) = tokio::fs::read(&file_path).await {
                                    use sha2::{Sha256, Digest};
                                    let mut hasher = Sha256::new();
                                    hasher.update(&data);
                                    let hash = format!("{:x}", hasher.finalize());
                                    
                                    let _ = sync_event_sender_clone.send(SyncEvent::SaveDetected {
                                        game_name: game_name.clone(),
                                        emulator,
                                        file_path,
                                        file_hash: hash,
                                        file_size: data.len() as i64,
                                    });
                                }
                            }
                        }
                        monitor::MonitorEvent::ManualSaveResult(result) => {
                            // Play audio feedback
                            audio_feedback_clone.play_save_result(&result);
                            
                            // Show colored terminal output
                            match &result {
                                monitor::SaveResult::Success { game_name, file_count } => {
                                    // Green success message
                                    println!("\n\x1b[32m✓ Save Successful!\x1b[0m");
                                    println!("  Game: {}", game_name);
                                    println!("  Files saved: {}", file_count);
                                    
                                    notif_manager_clone.show_success(
                                        "Save Successful", 
                                        &format!("{} - {} file(s) saved", game_name, file_count)
                                    );
                                }
                                monitor::SaveResult::NoChanges => {
                                    // Yellow info message
                                    println!("\n\x1b[33mℹ No changes to save\x1b[0m");
                                    
                                    notif_manager_clone.show_info(
                                        "No Changes", 
                                        "No save file changes detected"
                                    );
                                }
                                monitor::SaveResult::Failed(error) => {
                                    // Red error message
                                    println!("\n\x1b[31m✗ Save Failed!\x1b[0m");
                                    println!("  Error: {}", error);
                                    
                                    notif_manager_clone.show_error(
                                        "Save Failed", 
                                        error
                                    );
                                }
                            }
                        }
                    }
                }
                Some(tray_msg) = tray_receiver.recv() => {
                    debug!("Tray message received: {:?}", tray_msg);
                    // Handle tray-specific messages
                    match tray_msg {
                        ui::tray::TrayMessage::ManualSaveRequested => {
                            info!("Manual save requested by user");
                            
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_manual_save();
                            }
                            
                            let _ = cmd_sender_clone.send(monitor::MonitorCommand::TriggerManualSave).await;
                        }
                        ui::tray::TrayMessage::OpenSettings => {
                            info!("Opening settings window");
                            let settings_clone = settings_window_clone.clone();
                            // Show the settings window
                            tokio::spawn(async move {
                                if let Err(e) = settings_clone.show().await {
                                    error!("Failed to show settings window: {}", e);
                                }
                            });
                        }
                        ui::tray::TrayMessage::HotkeyChanged(new_hotkey) => {
                            info!("Hotkey changed to: {:?}", new_hotkey);
                            // Update the hotkey manager
                            if let Err(e) = hotkey_manager_clone.set_save_hotkey(new_hotkey) {
                                error!("Failed to update hotkey: {}", e);
                            }
                        }
                        ui::tray::TrayMessage::SyncStarted => {
                            info!("Cloud sync started");
                            tray.update_status("Syncing...");
                        }
                        ui::tray::TrayMessage::SyncCompleted { uploaded, downloaded } => {
                            info!("Cloud sync completed: {} uploaded, {} downloaded", uploaded, downloaded);
                            tray.update_status("Monitoring (synced)");
                            if uploaded > 0 || downloaded > 0 {
                                tray.show_notification(
                                    "Sync Complete", 
                                    &format!("↑{} ↓{} saves synced", uploaded, downloaded)
                                );
                            }
                        }
                        ui::tray::TrayMessage::SyncFailed(error) => {
                            error!("Cloud sync failed: {}", error);
                            tray.show_notification("Sync Failed", &error);
                        }
                        ui::tray::TrayMessage::CloudAuthChanged { is_authenticated, email } => {
                            if is_authenticated {
                                let msg = format!("Logged in as {}", email.as_deref().unwrap_or("unknown"));
                                info!("{}", msg);
                                tray.show_notification("Cloud Connected", &msg);
                            } else {
                                info!("Logged out from cloud");
                                tray.update_status("Monitoring (offline)");
                            }
                        }
                        _ => {
                            // Other messages are handled by the tray itself
                        }
                    }
                }
            }
        }
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("Shutting down Retrosave...");

    monitor_handle.abort();
    event_handle.abort();
    
    Ok(())
}