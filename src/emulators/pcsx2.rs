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
        if let Some(pid) = self.pid {
            // Try to get the actual game name from window title
            if let Some(game_name) = crate::monitor::process::get_pcsx2_game_name(pid) {
                return Some(game_name);
            }
            // Fallback to generic name if we can't get the actual title
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pcsx2_new() {
        let pcsx2 = PCSX2::new();
        assert_eq!(pcsx2.name(), "PCSX2");
        assert!(!pcsx2.is_running());
        assert_eq!(pcsx2.get_current_game(), None);
    }

    #[test]
    fn test_pcsx2_with_pid() {
        let pcsx2 = PCSX2::with_pid(1234);
        assert_eq!(pcsx2.name(), "PCSX2");
        assert!(pcsx2.is_running());
        assert_eq!(pcsx2.pid, Some(1234));
    }

    #[test]
    fn test_pcsx2_save_directory() {
        let mut pcsx2 = PCSX2::new();
        
        // Test with no save directory
        pcsx2.save_directory = None;
        assert_eq!(pcsx2.get_save_directory(), None);
        
        // Test with save directory
        pcsx2.save_directory = Some("/home/user/.config/PCSX2/memcards".to_string());
        assert_eq!(pcsx2.get_save_directory(), Some("/home/user/.config/PCSX2/memcards".to_string()));
    }

    #[test]
    fn test_is_running() {
        let mut pcsx2 = PCSX2::new();
        
        // Not running by default
        assert!(!pcsx2.is_running());
        
        // Running when PID is set
        pcsx2.pid = Some(5678);
        assert!(pcsx2.is_running());
        
        // Not running when PID is cleared
        pcsx2.pid = None;
        assert!(!pcsx2.is_running());
    }
}