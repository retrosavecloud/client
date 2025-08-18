use anyhow::Result;
use tracing::{info, error, debug};
use tracing_subscriber;
use tokio::sync::mpsc;

mod monitor;
mod emulators;
mod ui;
mod storage;

use ui::{SystemTray, tray::TrayMessage};
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
    tray.init()?;
    info!("System tray initialized");

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
    let tray_clone = tray.clone();
    let cmd_sender_clone = cmd_sender.clone();
    let event_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(event) = monitor_receiver.recv() => {
                    match event {
                        monitor::MonitorEvent::EmulatorStarted(name) => {
                            let msg = format!("{} detected", name);
                            tray_clone.update_status(&msg);
                            tray_clone.show_notification("Emulator Detected", &msg);
                            let _ = tray_clone.send_message(TrayMessage::EmulatorDetected(name)).await;
                        }
                        monitor::MonitorEvent::EmulatorStopped(name) => {
                            tray_clone.update_status("Monitoring");
                            tray_clone.show_notification("Emulator Stopped", &format!("{} has stopped", name));
                            let _ = tray_clone.send_message(TrayMessage::EmulatorStopped).await;
                        }
                        monitor::MonitorEvent::GameDetected(name) => {
                            tray_clone.update_status(&format!("Playing: {}", name));
                            let _ = tray_clone.send_message(TrayMessage::GameDetected(name)).await;
                        }
                        monitor::MonitorEvent::SaveDetected(game, path) => {
                            tray_clone.show_notification("Save Detected", &format!("{} saved", game));
                            let _ = tray_clone.send_message(TrayMessage::SaveDetected(format!("{}: {}", game, path))).await;
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