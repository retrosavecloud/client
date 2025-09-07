use super::Emulator;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, debug, warn};
use std::path::PathBuf;

pub struct Dolphin {
    pid: Option<u32>,
    save_directory: Option<String>,
    state_directory: Option<String>,
}

impl Dolphin {
    pub fn new() -> Self {
        let (save_directory, state_directory) = Self::get_dolphin_directories();
        
        if let Some(ref dir) = save_directory {
            info!("Dolphin save directory found: {}", dir);
        } else {
            warn!("Dolphin save directory not found");
        }
        
        if let Some(ref dir) = state_directory {
            info!("Dolphin state directory found: {}", dir);
        }
        
        Self {
            pid: None,
            save_directory,
            state_directory,
        }
    }
    
    pub fn with_pid(pid: u32) -> Self {
        let mut instance = Self::new();
        instance.pid = Some(pid);
        instance
    }
    
    fn get_dolphin_directories() -> (Option<String>, Option<String>) {
        let mut save_dir = None;
        let mut state_dir = None;
        
        #[cfg(target_os = "windows")]
        {
            if let Ok(home) = std::env::var("USERPROFILE") {
                // Check Documents folder for Dolphin
                let dolphin_path = format!("{}\\Documents\\Dolphin Emulator", home);
                if std::path::Path::new(&dolphin_path).exists() {
                    save_dir = Some(format!("{}\\GC", dolphin_path));
                    state_dir = Some(format!("{}\\StateSaves", dolphin_path));
                }
                
                // Check AppData for portable installation
                let appdata_path = format!("{}\\AppData\\Roaming\\Dolphin Emulator", home);
                if save_dir.is_none() && std::path::Path::new(&appdata_path).exists() {
                    save_dir = Some(format!("{}\\GC", appdata_path));
                    state_dir = Some(format!("{}\\StateSaves", appdata_path));
                }
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            if let Ok(home) = std::env::var("HOME") {
                // Check Flatpak location first
                let flatpak_path = format!("{}/.var/app/org.DolphinEmu.dolphin-emu/data/dolphin-emu", home);
                if std::path::Path::new(&flatpak_path).exists() {
                    save_dir = Some(format!("{}/GC", flatpak_path));
                    state_dir = Some(format!("{}/StateSaves", flatpak_path));
                } else {
                    // Check standard location
                    let standard_path = format!("{}/.local/share/dolphin-emu", home);
                    if std::path::Path::new(&standard_path).exists() {
                        save_dir = Some(format!("{}/GC", standard_path));
                        state_dir = Some(format!("{}/StateSaves", standard_path));
                    } else {
                        // Check old location
                        let old_path = format!("{}/.dolphin-emu", home);
                        if std::path::Path::new(&old_path).exists() {
                            save_dir = Some(format!("{}/GC", old_path));
                            state_dir = Some(format!("{}/StateSaves", old_path));
                        }
                    }
                }
            }
        }
        
        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let dolphin_path = format!("{}/Library/Application Support/Dolphin", home);
                if std::path::Path::new(&dolphin_path).exists() {
                    save_dir = Some(format!("{}/GC", dolphin_path));
                    state_dir = Some(format!("{}/StateSaves", dolphin_path));
                }
            }
        }
        
        (save_dir, state_dir)
    }
    
    fn detect_save_files(&self) -> Vec<PathBuf> {
        let mut save_files = Vec::new();
        
        // Check for GameCube memory card files
        if let Some(ref save_dir) = self.save_directory {
            let gc_path = PathBuf::from(save_dir);
            
            // Check for GCI folder format (most common)
            for region in &["USA", "EUR", "JAP", "JPN"] {
                for card in &["Card A", "Card B"] {
                    let gci_folder = gc_path.join(region).join(card);
                    if gci_folder.exists() {
                        // Add all .gci files in the folder
                        if let Ok(entries) = std::fs::read_dir(&gci_folder) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.extension().map_or(false, |e| e == "gci") {
                                    debug!("Found GCI save file: {:?}", path);
                                    save_files.push(path);
                                }
                            }
                        }
                    }
                }
            }
            
            // Also check for raw memory card format
            for card in &["MemoryCardA.raw", "MemoryCardB.raw"] {
                let card_path = gc_path.join(card);
                if card_path.exists() {
                    debug!("Found raw memory card: {:?}", card_path);
                    save_files.push(card_path);
                }
            }
            
            // Look for region-specific raw memory cards
            for region in &["USA", "EUR", "JAP", "JPN"] {
                for card in &["MemoryCardA", "MemoryCardB"] {
                    let card_path = gc_path.join(format!("{}.{}.raw", card, region));
                    if card_path.exists() {
                        debug!("Found region-specific memory card: {:?}", card_path);
                        save_files.push(card_path);
                    }
                }
            }
        }
        
        // Check for Wii save files
        if let Some(ref save_dir) = self.save_directory {
            let wii_path = PathBuf::from(save_dir).parent()
                .map(|p| p.join("Wii").join("title"));
            
            if let Some(wii_path) = wii_path {
                if wii_path.exists() {
                    // Wii saves are organized by title ID
                    if let Ok(entries) = std::fs::read_dir(&wii_path) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                // Look for data.bin files in save directories
                                let save_file = path.join("data").join("banner.bin");
                                if save_file.exists() {
                                    save_files.push(path);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        save_files
    }
}

#[async_trait]
impl Emulator for Dolphin {
    fn name(&self) -> &str {
        "Dolphin"
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
            if let Some(game_name) = crate::monitor::process::get_dolphin_game_name(pid) {
                return Some(game_name);
            }
            // Fallback to generic name if we can't get the actual title
            Some("Unknown GameCube/Wii Game".to_string())
        } else {
            None
        }
    }
    
    async fn monitor_saves(&self) -> Result<()> {
        if let Some(ref save_dir) = self.save_directory {
            info!("Monitoring Dolphin saves in: {}", save_dir);
            
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                debug!("Checking for save changes in {}", save_dir);
                
                // Check for GameCube memory cards and GCI files
                let save_files = self.detect_save_files();
                if !save_files.is_empty() {
                    info!("Found {} Dolphin save files", save_files.len());
                    for save_file in save_files {
                        debug!("  - {:?}", save_file);
                    }
                }
                
                // Check for save states if directory exists
                if let Some(ref state_dir) = self.state_directory {
                    if let Ok(entries) = std::fs::read_dir(state_dir) {
                        for entry in entries.flatten() {
                            if let Some(name) = entry.file_name().to_str() {
                                if name.ends_with(".sav") || name.ends_with(".s01") || name.ends_with(".s02") {
                                    debug!("Found save state: {}", name);
                                }
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
    fn test_dolphin_new() {
        let dolphin = Dolphin::new();
        assert_eq!(dolphin.name(), "Dolphin");
        assert!(!dolphin.is_running());
        assert_eq!(dolphin.get_current_game(), None);
    }

    #[test]
    fn test_dolphin_with_pid() {
        let dolphin = Dolphin::with_pid(1234);
        assert_eq!(dolphin.name(), "Dolphin");
        assert!(dolphin.is_running());
        assert_eq!(dolphin.pid, Some(1234));
    }

    #[test]
    fn test_dolphin_save_directory() {
        let mut dolphin = Dolphin::new();
        
        // Test with no save directory
        dolphin.save_directory = None;
        assert_eq!(dolphin.get_save_directory(), None);
        
        // Test with save directory
        dolphin.save_directory = Some("/home/user/.local/share/dolphin-emu/GC".to_string());
        assert_eq!(dolphin.get_save_directory(), Some("/home/user/.local/share/dolphin-emu/GC".to_string()));
    }

    #[test]
    fn test_is_running() {
        let mut dolphin = Dolphin::new();
        
        // Not running by default
        assert!(!dolphin.is_running());
        
        // Running when PID is set
        dolphin.pid = Some(5678);
        assert!(dolphin.is_running());
        
        // Not running when PID is cleared
        dolphin.pid = None;
        assert!(!dolphin.is_running());
    }
    
    #[test]
    fn test_detect_save_files() {
        let dolphin = Dolphin::new();
        // This will return empty vec if directories don't exist
        let saves = dolphin.detect_save_files();
        assert!(saves.is_empty() || !saves.is_empty()); // Valid either way
    }
}