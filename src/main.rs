use anyhow::Result;
use tracing::{info, error, debug};
use tracing_subscriber;
use tokio::sync::mpsc;

use retrosave::ui::{SystemTray, tray::TrayMessage, SettingsWindow, NotificationManager, AudioFeedback};
use retrosave::storage::{Database, SettingsManager};
use retrosave::hotkey::{HotkeyManager, HotkeyEvent};
use retrosave::sync::{AuthManager, SyncService, SyncEvent};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if it exists (for development)
    dotenv::dotenv().ok();
    
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
    
    // Create settings window with settings manager and auth manager (sync service will be added later)
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
    let (monitor_sender, mut monitor_receiver) = mpsc::channel::<retrosave::monitor::MonitorEvent>(100);
    let (cmd_sender, cmd_receiver) = mpsc::channel::<retrosave::monitor::MonitorCommand>(10);
    
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
    
    // Set sync service in settings window so it can trigger manual syncs
    settings_window.set_sync_service(sync_service.clone());
    
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
        if let Err(e) = retrosave::monitor::start_monitoring_with_sync(
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
                            let _ = cmd_sender_hotkey.send(retrosave::monitor::MonitorCommand::TriggerManualSave).await;
                        }
                    }
                }
                Some(event) = monitor_receiver.recv() => {
                    match event {
                        retrosave::monitor::MonitorEvent::EmulatorStarted(name) => {
                            let msg = format!("{} detected", name);
                            tray.update_status(&msg);
                            
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_emulator_detected(&name);
                            }
                            
                            tray.show_notification("Emulator Detected", &msg);
                            let _ = tray.send_message(TrayMessage::EmulatorDetected(name)).await;
                        }
                        retrosave::monitor::MonitorEvent::EmulatorStopped(name) => {
                            tray.update_status("Monitoring");
                            
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_emulator_stopped(&name);
                            }
                            
                            tray.show_notification("Emulator Stopped", &format!("{} has stopped", name));
                            let _ = tray.send_message(TrayMessage::EmulatorStopped).await;
                        }
                        retrosave::monitor::MonitorEvent::GameDetected(name) => {
                            tray.update_status(&format!("Playing: {}", name));
                            
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_game_detected(&name);
                            }
                            
                            let _ = tray.send_message(TrayMessage::GameDetected(name)).await;
                        }
                        retrosave::monitor::MonitorEvent::SaveDetected { game_name, emulator: _, file_path } => {
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_save_detected(&game_name);
                            }
                            
                            tray.show_notification("Save Detected", &format!("{} saved", game_name));
                            let _ = tray.send_message(TrayMessage::SaveDetected(format!("{}: {}", game_name, file_path))).await;
                            
                            // Note: The sync event is already sent from monitor/mod.rs when it detects a save
                            // No need to duplicate it here as it causes double uploads
                        }
                        retrosave::monitor::MonitorEvent::ManualSaveResult(result) => {
                            // Play audio feedback
                            audio_feedback_clone.play_save_result(&result);
                            
                            // Show colored terminal output
                            match &result {
                                retrosave::monitor::SaveResult::Success { game_name, file_count } => {
                                    // Green success message
                                    println!("\n\x1b[32m✓ Save Successful!\x1b[0m");
                                    println!("  Game: {}", game_name);
                                    println!("  Files saved: {}", file_count);
                                    
                                    notif_manager_clone.show_success(
                                        "Save Successful", 
                                        &format!("{} - {} file(s) saved", game_name, file_count)
                                    );
                                }
                                retrosave::monitor::SaveResult::NoChanges => {
                                    // Yellow info message
                                    println!("\n\x1b[33mℹ No changes to save\x1b[0m");
                                    
                                    notif_manager_clone.show_info(
                                        "No Changes", 
                                        "No save file changes detected"
                                    );
                                }
                                retrosave::monitor::SaveResult::Failed(error) => {
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
                        TrayMessage::ManualSaveRequested => {
                            info!("Manual save requested by user");
                            
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_manual_save();
                            }
                            
                            let _ = cmd_sender_clone.send(retrosave::monitor::MonitorCommand::TriggerManualSave).await;
                        }
                        TrayMessage::OpenDashboard => {
                            info!("Opening dashboard in browser");
                            
                            // Get the dashboard URL from environment or use default
                            let dashboard_url = std::env::var("RETROSAVE_WEB_URL")
                                .unwrap_or_else(|_| {
                                    if cfg!(debug_assertions) {
                                        "http://localhost:3000".to_string()
                                    } else {
                                        "https://retrosave.cloud".to_string()
                                    }
                                });
                            
                            let full_url = format!("{}/dashboard", dashboard_url);
                            
                            // Open in default browser
                            if let Err(e) = open::that(&full_url) {
                                error!("Failed to open dashboard in browser: {}", e);
                            } else {
                                info!("Opened dashboard at: {}", full_url);
                            }
                        }
                        TrayMessage::OpenSettings => {
                            info!("Opening settings window");
                            let settings_clone = settings_window_clone.clone();
                            // Show the settings window
                            tokio::spawn(async move {
                                if let Err(e) = settings_clone.show().await {
                                    error!("Failed to show settings window: {}", e);
                                }
                            });
                        }
                        TrayMessage::HotkeyChanged(new_hotkey) => {
                            info!("Hotkey changed to: {:?}", new_hotkey);
                            // Update the hotkey manager
                            if let Err(e) = hotkey_manager_clone.set_save_hotkey(new_hotkey) {
                                error!("Failed to update hotkey: {}", e);
                            }
                        }
                        TrayMessage::SyncStarted => {
                            info!("Cloud sync started");
                            tray.update_status("Syncing...");
                        }
                        TrayMessage::SyncCompleted { uploaded, downloaded } => {
                            info!("Cloud sync completed: {} uploaded, {} downloaded", uploaded, downloaded);
                            tray.update_status("Monitoring (synced)");
                            if uploaded > 0 || downloaded > 0 {
                                tray.show_notification(
                                    "Sync Complete", 
                                    &format!("↑{} ↓{} saves synced", uploaded, downloaded)
                                );
                            }
                        }
                        TrayMessage::SyncFailed(error) => {
                            error!("Cloud sync failed: {}", error);
                            tray.show_notification("Sync Failed", &error);
                        }
                        TrayMessage::CloudAuthChanged { is_authenticated, email } => {
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