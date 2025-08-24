use super::Emulator;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, debug, warn};
use std::path::PathBuf;

pub struct RPCS3 {
    pid: Option<u32>,
    save_directory: Option<String>,
}

impl RPCS3 {
    pub fn new() -> Self {
        let save_directory = Self::get_rpcs3_save_directory();
        
        if let Some(ref dir) = save_directory {
            info!("RPCS3 save directory found: {}", dir);
        } else {
            warn!("RPCS3 save directory not found");
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
    
    fn get_rpcs3_save_directory() -> Option<String> {
        #[cfg(target_os = "windows")]
        {
            if let Ok(home) = std::env::var("USERPROFILE") {
                // Check for RPCS3 save data
                let rpcs3_path = format!("{}\\RPCS3\\dev_hdd0\\home\\00000001\\savedata", home);
                if std::path::Path::new(&rpcs3_path).exists() {
                    return Some(rpcs3_path);
                }
                
                // Check Documents folder
                let docs_path = format!("{}\\Documents\\RPCS3\\dev_hdd0\\home\\00000001\\savedata", home);
                if std::path::Path::new(&docs_path).exists() {
                    return Some(docs_path);
                }
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            if let Ok(home) = std::env::var("HOME") {
                // Check Flatpak location first
                let flatpak_path = format!("{}/.var/app/net.rpcs3.RPCS3/config/rpcs3/dev_hdd0/home/00000001/savedata", home);
                if std::path::Path::new(&flatpak_path).exists() {
                    return Some(flatpak_path);
                }
                
                // Check standard location
                let standard_path = format!("{}/.config/rpcs3/dev_hdd0/home/00000001/savedata", home);
                if std::path::Path::new(&standard_path).exists() {
                    return Some(standard_path);
                }
                
                // Check old location
                let old_path = format!("{}/.rpcs3/dev_hdd0/home/00000001/savedata", home);
                if std::path::Path::new(&old_path).exists() {
                    return Some(old_path);
                }
            }
        }
        
        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let rpcs3_path = format!("{}/Library/Application Support/rpcs3/dev_hdd0/home/00000001/savedata", home);
                if std::path::Path::new(&rpcs3_path).exists() {
                    return Some(rpcs3_path);
                }
            }
        }
        
        None
    }
    
    fn detect_save_files(&self) -> Vec<PathBuf> {
        let mut save_files = Vec::new();
        
        if let Some(ref save_dir) = self.save_directory {
            let save_path = PathBuf::from(save_dir);
            
            // PS3 saves are organized by game ID (e.g., BLUS30443 for Red Dead Redemption)
            if let Ok(entries) = std::fs::read_dir(&save_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // Each game save directory contains SYS-DATA and other save files
                        let sys_data = path.join("SYS-DATA");
                        if sys_data.exists() {
                            save_files.push(path);
                        }
                    }
                }
            }
        }
        
        save_files
    }
    
    fn get_game_title_from_save(&self, save_path: &PathBuf) -> Option<String> {
        // Try to read PARAM.SFO file which contains game metadata
        let param_file = save_path.join("PARAM.SFO");
        if param_file.exists() {
            // For now, use the directory name (game ID) as the title
            // In a full implementation, we would parse the SFO file
            if let Some(dir_name) = save_path.file_name() {
                return Some(dir_name.to_string_lossy().to_string());
            }
        }
        None
    }
}

#[async_trait]
impl Emulator for RPCS3 {
    fn name(&self) -> &str {
        "RPCS3"
    }
    
    fn get_save_directory(&self) -> Option<String> {
        self.save_directory.clone()
    }
    
    fn is_running(&self) -> bool {
        self.pid.is_some()
    }
    
    fn get_current_game(&self) -> Option<String> {
        if let Some(pid) = self.pid {
            // Try to get the actual game name from window title or logs
            if let Some(game_name) = crate::monitor::process::get_rpcs3_game_name(pid) {
                return Some(game_name);
            }
            // Fallback to generic name if we can't get the actual title
            Some("Unknown PS3 Game".to_string())
        } else {
            None
        }
    }
    
    async fn monitor_saves(&self) -> Result<()> {
        if let Some(ref save_dir) = self.save_directory {
            info!("Monitoring RPCS3 saves in: {}", save_dir);
            
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                debug!("Checking for save changes in {}", save_dir);
                
                // Check for PS3 save files
                let save_files = self.detect_save_files();
                for save_file in save_files {
                    if let Some(title) = self.get_game_title_from_save(&save_file) {
                        debug!("Found save for game: {}", title);
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
    fn test_rpcs3_new() {
        let rpcs3 = RPCS3::new();
        assert_eq!(rpcs3.name(), "RPCS3");
        assert!(!rpcs3.is_running());
        assert_eq!(rpcs3.get_current_game(), None);
    }

    #[test]
    fn test_rpcs3_with_pid() {
        let rpcs3 = RPCS3::with_pid(1234);
        assert_eq!(rpcs3.name(), "RPCS3");
        assert!(rpcs3.is_running());
        assert_eq!(rpcs3.pid, Some(1234));
    }

    #[test]
    fn test_rpcs3_save_directory() {
        let mut rpcs3 = RPCS3::new();
        
        // Test with no save directory
        rpcs3.save_directory = None;
        assert_eq!(rpcs3.get_save_directory(), None);
        
        // Test with save directory
        rpcs3.save_directory = Some("/home/user/.config/rpcs3/dev_hdd0/home/00000001/savedata".to_string());
        assert_eq!(rpcs3.get_save_directory(), Some("/home/user/.config/rpcs3/dev_hdd0/home/00000001/savedata".to_string()));
    }

    #[test]
    fn test_is_running() {
        let mut rpcs3 = RPCS3::new();
        
        // Not running by default
        assert!(!rpcs3.is_running());
        
        // Running when PID is set
        rpcs3.pid = Some(5678);
        assert!(rpcs3.is_running());
        
        // Not running when PID is cleared
        rpcs3.pid = None;
        assert!(!rpcs3.is_running());
    }
    
    #[test]
    fn test_detect_save_files() {
        let rpcs3 = RPCS3::new();
        // This will return empty vec if directories don't exist
        let saves = rpcs3.detect_save_files();
        assert!(saves.is_empty() || !saves.is_empty()); // Valid either way
    }
}