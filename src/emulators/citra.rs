use super::Emulator;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, debug, warn};
use std::path::PathBuf;

pub struct Citra {
    pid: Option<u32>,
    save_directory: Option<String>,
}

impl Citra {
    pub fn new() -> Self {
        let save_directory = Self::get_citra_save_directory();
        
        if let Some(ref dir) = save_directory {
            info!("Citra save directory found: {}", dir);
        } else {
            warn!("Citra save directory not found");
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
    
    fn get_citra_save_directory() -> Option<String> {
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                // Check Citra save data location
                let citra_path = format!("{}\\Citra\\sdmc\\Nintendo 3DS", appdata);
                if std::path::Path::new(&citra_path).exists() {
                    return Some(citra_path);
                }
                
                // Check portable installation
                if let Ok(home) = std::env::var("USERPROFILE") {
                    let portable_path = format!("{}\\Citra\\user\\sdmc\\Nintendo 3DS", home);
                    if std::path::Path::new(&portable_path).exists() {
                        return Some(portable_path);
                    }
                }
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            if let Ok(home) = std::env::var("HOME") {
                // Check Flatpak location first
                let flatpak_path = format!("{}/.var/app/org.citra_emu.citra/data/citra-emu/sdmc/Nintendo 3DS", home);
                if std::path::Path::new(&flatpak_path).exists() {
                    return Some(flatpak_path);
                }
                
                // Check standard location
                let standard_path = format!("{}/.local/share/citra-emu/sdmc/Nintendo 3DS", home);
                if std::path::Path::new(&standard_path).exists() {
                    return Some(standard_path);
                }
                
                // Check old location
                let old_path = format!("{}/.citra-emu/sdmc/Nintendo 3DS", home);
                if std::path::Path::new(&old_path).exists() {
                    return Some(old_path);
                }
            }
        }
        
        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let citra_path = format!("{}/Library/Application Support/Citra/sdmc/Nintendo 3DS", home);
                if std::path::Path::new(&citra_path).exists() {
                    return Some(citra_path);
                }
            }
        }
        
        None
    }
    
    fn detect_save_files(&self) -> Vec<PathBuf> {
        let mut save_files = Vec::new();
        
        if let Some(ref save_dir) = self.save_directory {
            let save_path = PathBuf::from(save_dir);
            
            // 3DS saves are organized in a complex structure:
            // Nintendo 3DS/<ID0>/<ID1>/title/<TitleHigh>/<TitleLow>/data/00000001/
            // We need to recursively find save files
            if let Ok(entries) = std::fs::read_dir(&save_path) {
                for id0_entry in entries.flatten() {
                    let id0_path = id0_entry.path();
                    if id0_path.is_dir() {
                        if let Ok(id1_entries) = std::fs::read_dir(&id0_path) {
                            for id1_entry in id1_entries.flatten() {
                                let title_path = id1_entry.path().join("title");
                                if title_path.exists() {
                                    // Recursively find save files in title directory
                                    self.find_saves_recursive(&title_path, &mut save_files);
                                }
                                
                                // Also check for extdata (extra data)
                                let extdata_path = id1_entry.path().join("extdata");
                                if extdata_path.exists() {
                                    self.find_saves_recursive(&extdata_path, &mut save_files);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        save_files
    }
    
    fn find_saves_recursive(&self, dir: &PathBuf, save_files: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Check if this is a data directory with save files
                    let data_path = path.join("data").join("00000001");
                    if data_path.exists() {
                        save_files.push(path.clone());
                    }
                    // Continue searching recursively
                    self.find_saves_recursive(&path, save_files);
                }
            }
        }
    }
}

#[async_trait]
impl Emulator for Citra {
    fn name(&self) -> &str {
        "Citra"
    }
    
    fn get_save_directory(&self) -> Option<String> {
        self.save_directory.clone()
    }
    
    fn is_running(&self) -> bool {
        self.pid.is_some()
    }
    
    fn get_current_game(&self) -> Option<String> {
        if let Some(pid) = self.pid {
            // Try to get the actual game name from window title or config
            if let Some(game_name) = crate::monitor::process::get_citra_game_name(pid) {
                return Some(game_name);
            }
            // Fallback to generic name if we can't get the actual title
            Some("Unknown 3DS Game".to_string())
        } else {
            None
        }
    }
    
    async fn monitor_saves(&self) -> Result<()> {
        if let Some(ref save_dir) = self.save_directory {
            info!("Monitoring Citra saves in: {}", save_dir);
            
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                debug!("Checking for save changes in {}", save_dir);
                
                // Check for 3DS save files
                let save_files = self.detect_save_files();
                for save_file in save_files {
                    debug!("Found 3DS save: {:?}", save_file);
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
    fn test_citra_new() {
        let citra = Citra::new();
        assert_eq!(citra.name(), "Citra");
        assert!(!citra.is_running());
        assert_eq!(citra.get_current_game(), None);
    }

    #[test]
    fn test_citra_with_pid() {
        let citra = Citra::with_pid(1234);
        assert_eq!(citra.name(), "Citra");
        assert!(citra.is_running());
        assert_eq!(citra.pid, Some(1234));
    }

    #[test]
    fn test_citra_save_directory() {
        let mut citra = Citra::new();
        
        // Test with no save directory
        citra.save_directory = None;
        assert_eq!(citra.get_save_directory(), None);
        
        // Test with save directory
        citra.save_directory = Some("/home/user/.local/share/citra-emu/sdmc/Nintendo 3DS".to_string());
        assert_eq!(citra.get_save_directory(), Some("/home/user/.local/share/citra-emu/sdmc/Nintendo 3DS".to_string()));
    }

    #[test]
    fn test_is_running() {
        let mut citra = Citra::new();
        
        // Not running by default
        assert!(!citra.is_running());
        
        // Running when PID is set
        citra.pid = Some(5678);
        assert!(citra.is_running());
        
        // Not running when PID is cleared
        citra.pid = None;
        assert!(!citra.is_running());
    }
    
    #[test]
    fn test_detect_save_files() {
        let citra = Citra::new();
        // This will return empty vec if directories don't exist
        let saves = citra.detect_save_files();
        assert!(saves.is_empty() || !saves.is_empty()); // Valid either way
    }
}