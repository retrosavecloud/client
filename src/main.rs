use anyhow::Result;
use tracing::{info, error, debug, warn};
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
    
    // Initialize auth manager to load tokens from keyring and sync settings
    let mut synced_settings = saved_settings.clone();
    {
        let auth_manager_clone = auth_manager.clone();
        let settings_for_sync = saved_settings.clone();
        let settings_manager_for_sync = settings_manager.clone();
        
        // Use a channel to get the synced settings back
        let (settings_tx, mut settings_rx) = mpsc::channel::<retrosave::ui::settings::Settings>(1);
        
        tokio::spawn(async move {
            if let Err(e) = auth_manager_clone.init().await {
                error!("Failed to initialize auth manager: {}", e);
                // Send back original settings if auth fails
                let _ = settings_tx.send(settings_for_sync).await;
            } else {
                info!("Auth manager initialized, checking stored tokens...");
                
                // If authenticated, sync settings from cloud
                if auth_manager_clone.is_authenticated().await {
                    info!("User is authenticated, syncing settings from cloud...");
                    let api = retrosave::sync::api::SyncApi::new(
                        settings_for_sync.cloud_api_url.clone(),
                        auth_manager_clone.clone(),
                    );
                    
                    match retrosave::sync::settings_sync::sync_settings_from_cloud(&api, &settings_for_sync).await {
                        Ok(merged_settings) => {
                            info!("Settings synced from cloud");
                            // Save the synced settings to local database
                            if let Err(e) = settings_manager_for_sync.save_settings(&merged_settings).await {
                                error!("Failed to save synced settings: {}", e);
                            }
                            let _ = settings_tx.send(merged_settings).await;
                        }
                        Err(e) => {
                            error!("Failed to sync settings from cloud: {}", e);
                            let _ = settings_tx.send(settings_for_sync).await;
                        }
                    }
                } else {
                    info!("User not authenticated, using local settings");
                    let _ = settings_tx.send(settings_for_sync).await;
                }
            }
        });
        
        // Wait for settings sync (with timeout)
        if let Ok(Some(new_settings)) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            settings_rx.recv()
        ).await {
            synced_settings = new_settings;
            info!("Using synced settings");
        } else {
            info!("Settings sync timed out, using local settings");
        }
    }
    
    // Create settings window with settings manager and auth manager (sync service will be added later)
    let settings_window = Arc::new(SettingsWindow::with_auth_manager(
        synced_settings.clone(),  // Use synced settings instead of saved_settings
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
    let settings = synced_settings.clone();  // Use synced settings
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
        
        // Register for settings updates via WebSocket
        let sync_service_for_settings = sync_service.clone();
        let settings_manager_for_ws = settings_manager.clone();
        let settings_window_for_ws = settings_window.clone();
        tokio::spawn(async move {
            // Give sync service time to initialize
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            
            // Wait for WebSocket to be ready (up to 10 seconds)
            info!("Waiting for WebSocket to be ready for settings sync...");
            
            if let Some(event_handler) = sync_service_for_settings.wait_for_websocket(10).await {
                info!("WebSocket ready, registering for settings updates");
                event_handler.on_settings_update(move |cloud_settings| {
                    info!("Received settings update via WebSocket");
                    
                    // Get current local settings
                    let current_settings = settings_window_for_ws.get_settings();
                    
                    // Merge cloud settings with local settings
                    let merged = retrosave::sync::settings_sync::merge_settings(&current_settings, cloud_settings);
                    
                    // Update settings in window
                    settings_window_for_ws.update_settings(merged.clone());
                    
                    // Save to local database
                    let settings_manager = settings_manager_for_ws.clone();
                    tokio::spawn(async move {
                        if let Err(e) = settings_manager.save_settings(&merged).await {
                            error!("Failed to save WebSocket-updated settings: {}", e);
                        } else {
                            info!("Settings updated and saved from WebSocket");
                        }
                    });
                }).await;
                
                info!("Registered for WebSocket settings updates");
            } else {
                warn!("Could not register for settings updates - WebSocket not available");
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
    let _sync_event_sender_clone = sync_event_sender.clone();
    let sync_service_clone = sync_service.clone();
    
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
                            let _ = tray.send_message(TrayMessage::EmulatorDetected(name.clone())).await;
                            
                            // Trigger sync when emulator starts to ensure latest saves
                            if settings_window_clone.get_settings().cloud_sync_enabled {
                                info!("Triggering sync on {} start", name);
                                let sync_service = sync_service_clone.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = sync_service.trigger_sync().await {
                                        error!("Failed to trigger sync on emulator start: {}", e);
                                    }
                                });
                            }
                        }
                        retrosave::monitor::MonitorEvent::EmulatorStopped(name) => {
                            tray.update_status("Monitoring");
                            
                            // Show desktop notification if enabled
                            if settings_window_clone.get_settings().show_notifications {
                                notif_manager_clone.notify_emulator_stopped(&name);
                            }
                            
                            tray.show_notification("Emulator Stopped", &format!("{} has stopped", name));
                            let _ = tray.send_message(TrayMessage::EmulatorStopped).await;
                            
                            // Trigger sync when emulator stops to ensure all saves are uploaded
                            if settings_window_clone.get_settings().cloud_sync_enabled {
                                info!("Triggering sync on {} stop", name);
                                let sync_service = sync_service_clone.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = sync_service.trigger_sync().await {
                                        error!("Failed to trigger sync on emulator stop: {}", e);
                                    }
                                });
                            }
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
                            // The actual hotkey update is handled by the settings window
                            // This message is just for notification purposes
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