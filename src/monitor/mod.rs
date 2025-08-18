pub mod process;

use anyhow::Result;
use std::time::Duration;
use tokio::time;
use tracing::{info, debug};

pub async fn start_monitoring() -> Result<()> {
    info!("Process monitoring started");
    
    let mut interval = time::interval(Duration::from_secs(5));
    
    loop {
        interval.tick().await;
        
        // Check for running emulators
        if let Some(emulator) = process::detect_running_emulators() {
            debug!("Detected emulator: {:?}", emulator);
            
            // Handle the detected emulator
            match emulator {
                process::EmulatorProcess::PCSX2 { pid, exe_path } => {
                    info!("PCSX2 detected! PID: {}, Path: {}", pid, exe_path);
                    // TODO: Start monitoring PCSX2 save files
                }
            }
        }
    }
}