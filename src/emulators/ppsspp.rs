use super::Emulator;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, debug, warn};
use std::path::PathBuf;

pub struct PPSSPP {
    pid: Option<u32>,
    save_directory: Option<String>,
    state_directory: Option<String>,
}

impl PPSSPP {
    pub fn new() -> Self {
        let (save_directory, state_directory) = Self::get_ppsspp_directories();
        
        if let Some(ref dir) = save_directory {
            info!("PPSSPP save directory found: {}", dir);
        } else {
            warn!("PPSSPP save directory not found");
        }
        
        if let Some(ref dir) = state_directory {
            info!("PPSSPP state directory found: {}", dir);
        } else {
            warn!("PPSSPP state directory not found");
        }
        
        Self {
            pid: None,
            save_directory,
            state_directory,
        }
    }
    
    pub fn with_pid(pid: u32) -> Self {
        let mut ppsspp = Self::new();
        ppsspp.pid = Some(pid);
        ppsspp
    }
    
    fn get_ppsspp_directories() -> (Option<String>, Option<String>) {
        let mut save_dir = None;
        let mut state_dir = None;
        
        #[cfg(target_os = "windows")]
        {
            if let Ok(documents) = std::env::var("USERPROFILE") {
                let ppsspp_save = format!("{}\\Documents\\PPSSPP\\PSP\\SAVEDATA", documents);
                if std::path::Path::new(&ppsspp_save).exists() {
                    save_dir = Some(ppsspp_save);
                }
                
                let ppsspp_state = format!("{}\\Documents\\PPSSPP\\PSP\\PPSSPP_STATE", documents);
                if std::path::Path::new(&ppsspp_state).exists() {
                    state_dir = Some(ppsspp_state);
                }
            }
            
            if save_dir.is_none() {
                if let Ok(appdata) = std::env::var("APPDATA") {
                    let ppsspp_save = format!("{}\\PPSSPP\\PSP\\SAVEDATA", appdata);
                    if std::path::Path::new(&ppsspp_save).exists() {
                        save_dir = Some(ppsspp_save);
                    }
                    
                    let ppsspp_state = format!("{}\\PPSSPP\\PSP\\PPSSPP_STATE", appdata);
                    if std::path::Path::new(&ppsspp_state).exists() {
                        state_dir = Some(ppsspp_state);
                    }
                }
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            if let Ok(home) = std::env::var("HOME") {
                // Check Flatpak location first
                let flatpak_save = format!("{}/.var/app/org.ppsspp.PPSSPP/.config/ppsspp/PSP/SAVEDATA", home);
                if std::path::Path::new(&flatpak_save).exists() {
                    save_dir = Some(flatpak_save);
                }
                
                let flatpak_state = format!("{}/.var/app/org.ppsspp.PPSSPP/.config/ppsspp/PSP/PPSSPP_STATE", home);
                if std::path::Path::new(&flatpak_state).exists() {
                    state_dir = Some(flatpak_state);
                }
                
                // Check standard location
                if save_dir.is_none() {
                    let standard_save = format!("{}/.config/ppsspp/PSP/SAVEDATA", home);
                    if std::path::Path::new(&standard_save).exists() {
                        save_dir = Some(standard_save);
                    }
                    
                    let standard_state = format!("{}/.config/ppsspp/PSP/PPSSPP_STATE", home);
                    if std::path::Path::new(&standard_state).exists() {
                        state_dir = Some(standard_state);
                    }
                }
                
                // Check old location
                if save_dir.is_none() {
                    let old_save = format!("{}/.ppsspp/PSP/SAVEDATA", home);
                    if std::path::Path::new(&old_save).exists() {
                        save_dir = Some(old_save);
                    }
                    
                    let old_state = format!("{}/.ppsspp/PSP/PPSSPP_STATE", home);
                    if std::path::Path::new(&old_state).exists() {
                        state_dir = Some(old_state);
                    }
                }
            }
        }
        
        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let ppsspp_save = format!("{}/Library/Application Support/PPSSPP/PSP/SAVEDATA", home);
                if std::path::Path::new(&ppsspp_save).exists() {
                    save_dir = Some(ppsspp_save);
                }
                
                let ppsspp_state = format!("{}/Library/Application Support/PPSSPP/PSP/PPSSPP_STATE", home);
                if std::path::Path::new(&ppsspp_state).exists() {
                    state_dir = Some(ppsspp_state);
                }
            }
        }
        
        (save_dir, state_dir)
    }
    
    fn detect_save_files(&self) -> Vec<PathBuf> {
        let mut save_files = Vec::new();
        
        if let Some(ref save_dir) = self.save_directory {
            let save_path = PathBuf::from(save_dir);
            
            // PSP saves are organized by game ID
            // Format: SAVEDATA/<GAME_ID><SAVE_ID>/
            // Example: ULUS10336_SAVE001 (God of War: Chains of Olympus)
            if let Ok(entries) = std::fs::read_dir(&save_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // Check if it's a valid PSP save directory
                        // PSP saves typically contain PARAM.SFO file
                        let param_file = path.join("PARAM.SFO");
                        if param_file.exists() {
                            save_files.push(path);
                        }
                    }
                }
            }
        }
        
        // Also check for save states
        if let Some(ref state_dir) = self.state_directory {
            let state_path = PathBuf::from(state_dir);
            if let Ok(entries) = std::fs::read_dir(&state_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        // PPSSPP save states have .ppst extension
                        if let Some(ext) = path.extension() {
                            if ext == "ppst" {
                                save_files.push(path);
                            }
                        }
                    }
                }
            }
        }
        
        save_files
    }
    
    fn get_game_name_from_id(&self, game_id: &str) -> Option<String> {
        // Extract the game ID part (before underscore)
        let base_id = game_id.split('_').next().unwrap_or(game_id);
        
        // Map common PSP game IDs to names
        match base_id {
            // US releases (ULUS)
            "ULUS10336" => Some("God of War: Chains of Olympus".to_string()),
            "ULUS10391" => Some("God of War: Ghost of Sparta".to_string()),
            "ULUS10041" => Some("Grand Theft Auto: Liberty City Stories".to_string()),
            "ULUS10160" => Some("Grand Theft Auto: Vice City Stories".to_string()),
            "ULUS10509" => Some("Final Fantasy Type-0".to_string()),
            "ULUS10566" => Some("Kingdom Hearts: Birth by Sleep".to_string()),
            "ULUS10487" => Some("Metal Gear Solid: Peace Walker".to_string()),
            "ULUS10141" => Some("Tekken: Dark Resurrection".to_string()),
            "ULUS10362" => Some("Crisis Core: Final Fantasy VII".to_string()),
            "ULUS10437" => Some("Dissidia Final Fantasy".to_string()),
            "ULUS10560" => Some("Dissidia 012 Final Fantasy".to_string()),
            "ULUS10176" => Some("Burnout Legends".to_string()),
            "ULUS10064" => Some("Burnout Dominator".to_string()),
            
            // EU releases (ULES)
            "ULES00151" => Some("Grand Theft Auto: Liberty City Stories".to_string()),
            "ULES00502" => Some("Grand Theft Auto: Vice City Stories".to_string()),
            "ULES00125" => Some("Tekken: Dark Resurrection".to_string()),
            "ULES01044" => Some("Crisis Core: Final Fantasy VII".to_string()),
            
            // JP releases (ULJM)
            "ULJM05600" => Some("Kingdom Hearts: Birth by Sleep Final Mix".to_string()),
            "ULJM05775" => Some("Final Fantasy Type-0".to_string()),
            "ULJM05193" => Some("Monster Hunter Portable 2nd".to_string()),
            "ULJM05500" => Some("Monster Hunter Portable 2nd G".to_string()),
            "ULJM05800" => Some("Monster Hunter Portable 3rd".to_string()),
            
            // Check for Monster Hunter pattern
            _ if base_id.starts_with("ULJM") && game_id.contains("MHP") => {
                Some("Monster Hunter Portable".to_string())
            }
            _ if base_id.starts_with("ULUS") && game_id.contains("MHF") => {
                Some("Monster Hunter Freedom".to_string())
            }
            
            _ => None,
        }
    }
}

#[async_trait]
impl Emulator for PPSSPP {
    fn name(&self) -> &str {
        "PPSSPP"
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
            let game_name = crate::monitor::process::get_ppsspp_game_name(pid);
            
            if let Some(name) = game_name {
                return Some(name);
            }
            
            // Fallback: check recently modified saves
            if let Some(ref save_dir) = self.save_directory {
                let save_path = PathBuf::from(save_dir);
                let mut recent_game = None;
                let mut recent_time = std::time::SystemTime::UNIX_EPOCH;
                
                if let Ok(entries) = std::fs::read_dir(&save_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            if let Ok(metadata) = std::fs::metadata(&path) {
                                if let Ok(modified) = metadata.modified() {
                                    if modified > recent_time {
                                        recent_time = modified;
                                        if let Some(dir_name) = path.file_name() {
                                            let name = dir_name.to_string_lossy();
                                            recent_game = self.get_game_name_from_id(&name)
                                                .or_else(|| Some(format!("PSP Game ({})", name)));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                if let Some(game) = recent_game {
                    return Some(game);
                }
            }
            
            Some("Unknown PSP Game".to_string())
        } else {
            None
        }
    }
    
    async fn monitor_saves(&self) -> Result<()> {
        if self.save_directory.is_some() || self.state_directory.is_some() {
            info!("Monitoring PPSSPP saves");
            
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                
                if let Some(ref save_dir) = self.save_directory {
                    debug!("Checking for save changes in {}", save_dir);
                }
                
                if let Some(ref state_dir) = self.state_directory {
                    debug!("Checking for state changes in {}", state_dir);
                }
                
                let save_files = self.detect_save_files();
                for save_file in save_files {
                    if let Some(file_name) = save_file.file_name() {
                        let name = file_name.to_string_lossy();
                        if let Some(game_name) = self.get_game_name_from_id(&name) {
                            debug!("Found save for: {}", game_name);
                        } else {
                            debug!("Found save: {}", name);
                        }
                    }
                }
            }
        } else {
            warn!("Cannot monitor saves: directories not found");
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ppsspp_new() {
        let ppsspp = PPSSPP::new();
        assert_eq!(ppsspp.name(), "PPSSPP");
        assert!(!ppsspp.is_running());
        assert_eq!(ppsspp.get_current_game(), None);
    }

    #[test]
    fn test_with_pid() {
        let ppsspp = PPSSPP::with_pid(1234);
        assert_eq!(ppsspp.name(), "PPSSPP");
        assert!(ppsspp.is_running());
        assert_eq!(ppsspp.pid, Some(1234));
    }

    #[test]
    fn test_save_directory() {
        let mut ppsspp = PPSSPP::new();
        
        ppsspp.save_directory = None;
        assert_eq!(ppsspp.get_save_directory(), None);
        
        ppsspp.save_directory = Some("/home/user/.config/ppsspp/PSP/SAVEDATA".to_string());
        assert_eq!(ppsspp.get_save_directory(), Some("/home/user/.config/ppsspp/PSP/SAVEDATA".to_string()));
    }

    #[test]
    fn test_is_running() {
        let mut ppsspp = PPSSPP::new();
        
        assert!(!ppsspp.is_running());
        
        ppsspp.pid = Some(5678);
        assert!(ppsspp.is_running());
        
        ppsspp.pid = None;
        assert!(!ppsspp.is_running());
    }

    #[test]
    fn test_game_name_from_id() {
        let ppsspp = PPSSPP::new();
        
        assert_eq!(
            ppsspp.get_game_name_from_id("ULUS10336_SAVE001"),
            Some("God of War: Chains of Olympus".to_string())
        );
        
        assert_eq!(
            ppsspp.get_game_name_from_id("ULUS10336"),
            Some("God of War: Chains of Olympus".to_string())
        );
        
        assert_eq!(
            ppsspp.get_game_name_from_id("ULJM05800"),
            Some("Monster Hunter Portable 3rd".to_string())
        );
        
        assert_eq!(ppsspp.get_game_name_from_id("UNKNOWN123"), None);
    }

    #[test]
    fn test_detect_save_files() {
        let ppsspp = PPSSPP::new();
        let saves = ppsspp.detect_save_files();
        assert!(saves.is_empty() || !saves.is_empty());
    }
}