pub mod process;

use anyhow::Result;
use std::time::Duration;
use std::collections::HashSet;
use tokio::time;
use tokio::sync::mpsc;
use tracing::{info, debug};

#[derive(Debug, Clone)]
pub enum MonitorEvent {
    EmulatorStarted(String),
    EmulatorStopped(String),
    GameDetected(String),
}

pub async fn start_monitoring() -> Result<()> {
    let (sender, _receiver) = mpsc::channel(100);
    start_monitoring_with_sender(sender).await
}

pub async fn start_monitoring_with_sender(sender: mpsc::Sender<MonitorEvent>) -> Result<()> {
    info!("Process monitoring started");
    
    let mut interval = time::interval(Duration::from_secs(5));
    let mut tracked_emulators = HashSet::new();
    
    loop {
        interval.tick().await;
        
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
                
                // Try to detect the game
                // TODO: Implement actual game detection
                tokio::time::sleep(Duration::from_secs(2)).await;
                let _ = sender.send(MonitorEvent::GameDetected("Unknown Game".to_string())).await;
            }
            
            match emulator {
                process::EmulatorProcess::PCSX2 { pid, exe_path } => {
                    debug!("PCSX2 running - PID: {}, Path: {}", pid, exe_path);
                    // TODO: Monitor save files
                }
            }
        } else {
            // Check if any tracked emulator has stopped
            if !tracked_emulators.is_empty() {
                for emulator in tracked_emulators.drain() {
                    info!("{} stopped", emulator);
                    let _ = sender.send(MonitorEvent::EmulatorStopped(emulator)).await;
                }
            }
        }
    }
}