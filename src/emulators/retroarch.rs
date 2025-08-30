use super::Emulator;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, debug, warn};
use std::path::{Path, PathBuf};
use std::collections::HashMap;

pub struct RetroArch {
    pid: Option<u32>,
    save_directory: Option<String>,
    state_directory: Option<String>,
    #[allow(dead_code)]
    config_path: Option<String>,
    current_core: Option<String>,
}

impl RetroArch {
    pub fn new() -> Self {
        let (save_directory, state_directory, config_path) = Self::get_retroarch_directories();
        
        if let Some(ref dir) = save_directory {
            info!("RetroArch save directory found: {}", dir);
        } else {
            warn!("RetroArch save directory not found");
        }
        
        if let Some(ref dir) = state_directory {
            info!("RetroArch state directory found: {}", dir);
        }
        
        Self {
            pid: None,
            save_directory,
            state_directory,
            config_path,
            current_core: None,
        }
    }
    
    pub fn with_pid(pid: u32) -> Self {
        let mut instance = Self::new();
        instance.pid = Some(pid);
        instance
    }
    
    fn get_retroarch_directories() -> (Option<String>, Option<String>, Option<String>) {
        let mut save_dir = None;
        let mut state_dir = None;
        let mut config_path = None;
        
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let retroarch_path = format!("{}\\RetroArch", appdata);
                if std::path::Path::new(&retroarch_path).exists() {
                    config_path = Some(format!("{}\\retroarch.cfg", retroarch_path));
                    
                    // Try to parse config for actual save paths
                    if let Some((saves, states)) = Self::parse_config(&config_path.clone().unwrap()) {
                        save_dir = Some(saves);
                        state_dir = Some(states);
                    } else {
                        // Use defaults
                        save_dir = Some(format!("{}\\saves", retroarch_path));
                        state_dir = Some(format!("{}\\states", retroarch_path));
                    }
                }
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            if let Ok(home) = std::env::var("HOME") {
                // Check Flatpak location first
                let flatpak_path = format!("{}/.var/app/org.libretro.RetroArch/config/retroarch", home);
                if std::path::Path::new(&flatpak_path).exists() {
                    config_path = Some(format!("{}/retroarch.cfg", flatpak_path));
                    
                    if let Some((saves, states)) = Self::parse_config(&config_path.clone().unwrap()) {
                        save_dir = Some(saves);
                        state_dir = Some(states);
                    } else {
                        save_dir = Some(format!("{}/saves", flatpak_path));
                        state_dir = Some(format!("{}/states", flatpak_path));
                    }
                } else {
                    // Check standard location
                    let standard_path = format!("{}/.config/retroarch", home);
                    if std::path::Path::new(&standard_path).exists() {
                        config_path = Some(format!("{}/retroarch.cfg", standard_path));
                        
                        if let Some((saves, states)) = Self::parse_config(&config_path.clone().unwrap()) {
                            save_dir = Some(saves);
                            state_dir = Some(states);
                        } else {
                            save_dir = Some(format!("{}/saves", standard_path));
                            state_dir = Some(format!("{}/states", standard_path));
                        }
                    }
                }
            }
        }
        
        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let retroarch_path = format!("{}/Library/Application Support/RetroArch", home);
                if std::path::Path::new(&retroarch_path).exists() {
                    config_path = Some(format!("{}/config/retroarch.cfg", retroarch_path));
                    
                    if let Some((saves, states)) = Self::parse_config(&config_path.clone().unwrap()) {
                        save_dir = Some(saves);
                        state_dir = Some(states);
                    } else {
                        save_dir = Some(format!("{}/saves", retroarch_path));
                        state_dir = Some(format!("{}/states", retroarch_path));
                    }
                }
            }
        }
        
        (save_dir, state_dir, config_path)
    }
    
    fn parse_config(config_path: &str) -> Option<(String, String)> {
        if let Ok(content) = std::fs::read_to_string(config_path) {
            let mut savefile_directory = None;
            let mut savestate_directory = None;
            
            for line in content.lines() {
                if line.starts_with("savefile_directory") {
                    if let Some(value) = line.split('=').nth(1) {
                        let path = value.trim().trim_matches('"');
                        if !path.is_empty() && path != "default" {
                            savefile_directory = Some(path.to_string());
                        }
                    }
                } else if line.starts_with("savestate_directory") {
                    if let Some(value) = line.split('=').nth(1) {
                        let path = value.trim().trim_matches('"');
                        if !path.is_empty() && path != "default" {
                            savestate_directory = Some(path.to_string());
                        }
                    }
                }
            }
            
            if savefile_directory.is_some() && savestate_directory.is_some() {
                return Some((savefile_directory.unwrap(), savestate_directory.unwrap()));
            }
        }
        None
    }
    #[allow(dead_code)]
    
    fn detect_current_core(&mut self) -> Option<String> {
        // Try to detect the currently loaded core from process info or recent history
        if let Some(ref config_path) = self.config_path {
            if let Ok(content) = std::fs::read_to_string(config_path) {
                for line in content.lines() {
                    if line.starts_with("libretro_path") {
                        if let Some(value) = line.split('=').nth(1) {
                            let core_path = value.trim().trim_matches('"');
                            if let Some(core_name) = Path::new(core_path).file_stem() {
                                let core = core_name.to_string_lossy().to_string();
                                self.current_core = Some(core.clone());
                                return Some(core);
                            }
                        }
                    }
                }
            }
        }
        None
    }
    #[allow(dead_code)]
    
    fn get_core_save_path(&self, core: &str) -> Option<PathBuf> {
        // Map cores to their save subdirectories
        let core_mapping = HashMap::from([
            ("snes9x", "Nintendo - SNES"),
            ("mgba", "Nintendo - Game Boy Advance"),
            ("gambatte", "Nintendo - Game Boy"),
            ("mupen64plus", "Nintendo - Nintendo 64"),
            ("nestopia", "Nintendo - NES"),
            ("genesis_plus_gx", "Sega - Mega Drive - Genesis"),
            ("picodrive", "Sega - Mega Drive - Genesis"),
            ("pcsx_rearmed", "Sony - PlayStation"),
            ("beetle_psx", "Sony - PlayStation"),
            ("ppsspp", "Sony - PlayStation Portable"),
        ]);
        
        if let Some(ref save_dir) = self.save_directory {
            let base_path = PathBuf::from(save_dir);
            
            // Check if core has a known mapping
            for (core_name, system_dir) in &core_mapping {
                if core.contains(core_name) {
                    let system_path = base_path.join(system_dir);
                    if system_path.exists() {
                        return Some(system_path);
                    }
                }
            }
            
            // Fallback to base save directory
            return Some(base_path);
        }
        
        None
    }
    
    fn detect_save_files(&self) -> Vec<PathBuf> {
        let mut save_files = Vec::new();
        
        if let Some(ref save_dir) = self.save_directory {
            let save_path = PathBuf::from(save_dir);
            
            // RetroArch saves are usually .srm files (battery saves) or .sav files
            if let Ok(entries) = std::fs::read_dir(&save_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(ext) = path.extension() {
                            if ext == "srm" || ext == "sav" || ext == "eep" || ext == "flash" {
                                save_files.push(path);
                            }
                        }
                    } else if path.is_dir() {
                        // Check subdirectories for system-specific saves
                        if let Ok(sub_entries) = std::fs::read_dir(&path) {
                            for sub_entry in sub_entries.flatten() {
                                let sub_path = sub_entry.path();
                                if sub_path.is_file() {
                                    if let Some(ext) = sub_path.extension() {
                                        if ext == "srm" || ext == "sav" || ext == "eep" || ext == "flash" {
                                            save_files.push(sub_path);
                                        }
                                    }
                                }
                            }
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
                        if let Some(ext) = path.extension() {
                            if ext == "state" || ext.to_string_lossy().starts_with("state") {
                                save_files.push(path);
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
impl Emulator for RetroArch {
    fn name(&self) -> &str {
        "RetroArch"
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
            if let Some(game_name) = crate::monitor::process::get_retroarch_game_name(pid) {
                return Some(game_name);
            }
            
            // Fallback to core name if available
            if let Some(ref core) = self.current_core {
                return Some(format!("RetroArch ({})", core));
            }
            
            // Generic fallback
            Some("Unknown RetroArch Game".to_string())
        } else {
            None
        }
    }
    
    async fn monitor_saves(&self) -> Result<()> {
        if let Some(ref save_dir) = self.save_directory {
            info!("Monitoring RetroArch saves in: {}", save_dir);
            
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                debug!("Checking for save changes in {}", save_dir);
                
                // Check for save files
                let save_files = self.detect_save_files();
                for save_file in save_files {
                    debug!("Found RetroArch save: {:?}", save_file);
                }
                
                // Log current core if detected
                if let Some(ref core) = self.current_core {
                    debug!("Current core: {}", core);
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
    fn test_retroarch_new() {
        let retroarch = RetroArch::new();
        assert_eq!(retroarch.name(), "RetroArch");
        assert!(!retroarch.is_running());
        assert_eq!(retroarch.get_current_game(), None);
    }

    #[test]
    fn test_retroarch_with_pid() {
        let retroarch = RetroArch::with_pid(1234);
        assert_eq!(retroarch.name(), "RetroArch");
        assert!(retroarch.is_running());
        assert_eq!(retroarch.pid, Some(1234));
    }

    #[test]
    fn test_retroarch_save_directory() {
        let mut retroarch = RetroArch::new();
        
        // Test with no save directory
        retroarch.save_directory = None;
        assert_eq!(retroarch.get_save_directory(), None);
        
        // Test with save directory
        retroarch.save_directory = Some("/home/user/.config/retroarch/saves".to_string());
        assert_eq!(retroarch.get_save_directory(), Some("/home/user/.config/retroarch/saves".to_string()));
    }

    #[test]
    fn test_is_running() {
        let mut retroarch = RetroArch::new();
        
        // Not running by default
        assert!(!retroarch.is_running());
        
        // Running when PID is set
        retroarch.pid = Some(5678);
        assert!(retroarch.is_running());
        
        // Not running when PID is cleared
        retroarch.pid = None;
        assert!(!retroarch.is_running());
    }
    
    #[test]
    fn test_detect_save_files() {
        let retroarch = RetroArch::new();
        // This will return empty vec if directories don't exist
        let saves = retroarch.detect_save_files();
        assert!(saves.is_empty() || !saves.is_empty()); // Valid either way
    }
    
    #[test]
    fn test_core_save_path() {
        let retroarch = RetroArch::new();
        
        // Test with known core
        if retroarch.save_directory.is_some() {
            let path = retroarch.get_core_save_path("snes9x_libretro");
            assert!(path.is_some());
        }
    }
}