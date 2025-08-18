use anyhow::Result;
use tracing::{info, error, debug};
use tracing_subscriber;
use tokio::sync::mpsc;

mod monitor;
mod emulators;
mod ui;

use ui::{SystemTray, tray::TrayMessage};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("retrosave=debug")
        .init();

    info!("Starting Retrosave...");

    // Initialize system tray
    let (tray, mut tray_receiver) = SystemTray::new()?;
    tray.init()?;
    info!("System tray initialized");

    // Create channel for monitor to tray communication
    let (monitor_sender, mut monitor_receiver) = mpsc::channel::<monitor::MonitorEvent>(100);

    // Start process monitoring with sender
    let monitor_handle = tokio::spawn(async move {
        if let Err(e) = monitor::start_monitoring_with_sender(monitor_sender).await {
            error!("Monitor error: {}", e);
        }
    });

    // Handle monitor events and update tray
    let tray_clone = tray.clone();
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
                    }
                }
                Some(tray_msg) = tray_receiver.recv() => {
                    debug!("Tray message received: {:?}", tray_msg);
                    // Handle tray-specific messages if needed
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