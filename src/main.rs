use anyhow::Result;
use tracing::{info, error, debug};
use tracing_subscriber;
use tokio::sync::mpsc;

mod monitor;
mod emulators;
mod ui;
mod storage;
mod hotkey;

use ui::{SystemTray, tray::TrayMessage, SettingsWindow, NotificationManager, AudioFeedback};
use storage::{Database, SettingsManager};
use hotkey::{HotkeyManager, HotkeyEvent};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("retrosave=debug")
        .init();

    info!("Starting Retrosave...");

    // Initialize database
    let db = Arc::new(Database::new(None).await?);
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

    // Create settings window with loaded settings
    let settings_window = Arc::new(SettingsWindow::new_with_settings(saved_settings)?);
    
    // Set up callback to save settings when changed
    let settings_manager_clone = settings_manager.clone();
    settings_window.set_save_callback(Box::new(move |settings| {
        let manager = settings_manager_clone.clone();
        tokio::spawn(async move {
            if let Err(e) = manager.save_settings(&settings).await {
                error!("Failed to save settings: {}", e);
            }
        });
    }));

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

    // Start process monitoring with database and command channel
    let db_clone = db.clone();
    let monitor_handle = tokio::spawn(async move {
        if let Err(e) = monitor::start_monitoring_with_commands(monitor_sender, db_clone, cmd_receiver).await {
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
                            tray.show_notification("Emulator Detected", &msg);
                            let _ = tray.send_message(TrayMessage::EmulatorDetected(name)).await;
                        }
                        monitor::MonitorEvent::EmulatorStopped(name) => {
                            tray.update_status("Monitoring");
                            tray.show_notification("Emulator Stopped", &format!("{} has stopped", name));
                            let _ = tray.send_message(TrayMessage::EmulatorStopped).await;
                        }
                        monitor::MonitorEvent::GameDetected(name) => {
                            tray.update_status(&format!("Playing: {}", name));
                            let _ = tray.send_message(TrayMessage::GameDetected(name)).await;
                        }
                        monitor::MonitorEvent::SaveDetected(game, path) => {
                            tray.show_notification("Save Detected", &format!("{} saved", game));
                            let _ = tray.send_message(TrayMessage::SaveDetected(format!("{}: {}", game, path))).await;
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