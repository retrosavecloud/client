use anyhow::Result;
use tracing::{info, error, debug};
use tracing_subscriber;
use tokio::sync::mpsc;

mod monitor;
mod emulators;
mod ui;
mod storage;

use ui::{SystemTray, tray::TrayMessage, SettingsWindow, NotificationManager};
use storage::Database;
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

    // Initialize system tray
    let (tray, mut tray_receiver) = SystemTray::new()?;
    info!("System tray initialized");

    // Create settings window (but don't show it yet)
    let settings_window = Arc::new(SettingsWindow::new());

    // Create notification manager
    let notif_manager = Arc::new(NotificationManager::new());

    // Create channels for monitor communication
    let (monitor_sender, mut monitor_receiver) = mpsc::channel::<monitor::MonitorEvent>(100);
    let (cmd_sender, cmd_receiver) = mpsc::channel::<monitor::MonitorCommand>(10);

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
    let _notif_manager_clone = notif_manager.clone();
    let event_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
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
                            // Run settings window in a separate thread
                            std::thread::spawn(move || {
                                settings_clone.show();
                                if let Err(e) = settings_clone.run() {
                                    error!("Failed to run settings window: {}", e);
                                }
                            });
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