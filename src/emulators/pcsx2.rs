use super::Emulator;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, debug, warn};

pub struct PCSX2 {
    pid: Option<u32>,
    save_directory: Option<String>,
}

impl PCSX2 {
    pub fn new() -> Self {
        let save_directory = crate::monitor::process::get_pcsx2_save_directory();
        
        if let Some(ref dir) = save_directory {
            info!("PCSX2 save directory found: {}", dir);
        } else {
            warn!("PCSX2 save directory not found");
        }
        
        Self {
            pid: None,
            save_directory,
        }
    }
    
    pub fn with_pid(pid: u32) -> Self {
        let mut instance = Self::new();
        instance.pid = Some(pid);
        instance
    }
}

#[async_trait]
impl Emulator for PCSX2 {
    fn name(&self) -> &str {
        "PCSX2"
    }
    
    fn get_save_directory(&self) -> Option<String> {
        self.save_directory.clone()
    }
    
    fn is_running(&self) -> bool {
        self.pid.is_some()
    }
    
    fn get_current_game(&self) -> Option<String> {
        // TODO: Parse PCSX2 window title or logs to get game name
        // For now, return a placeholder
        if self.is_running() {
            Some("Unknown PS2 Game".to_string())
        } else {
            None
        }
    }
    
    async fn monitor_saves(&self) -> Result<()> {
        if let Some(ref save_dir) = self.save_directory {
            info!("Monitoring PCSX2 saves in: {}", save_dir);
            
            // TODO: Implement file watching with notify crate
            // For now, just log that we're monitoring
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                debug!("Checking for save changes in {}", save_dir);
                
                // List memory card files
                if let Ok(entries) = std::fs::read_dir(save_dir) {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            if name.ends_with(".ps2") || name.contains("Mcd") {
                                debug!("Found memory card: {}", name);
                            }
                        }
                    }
                }
            }
        } else {
            warn!("Cannot monitor saves: directory not found");
        }
        
        Ok(())
    }
}