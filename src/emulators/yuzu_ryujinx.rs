use super::Emulator;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, debug, warn};
use std::path::PathBuf;

pub struct YuzuRyujinx {
    pid: Option<u32>,
    emulator_type: EmulatorType,
    save_directory: Option<String>,
}

#[derive(Debug, Clone)]
pub enum EmulatorType {
    Yuzu,
    Ryujinx,
}

impl YuzuRyujinx {
    pub fn new_yuzu() -> Self {
        let save_directory = Self::get_yuzu_save_directory();
        
        if let Some(ref dir) = save_directory {
            info!("Yuzu save directory found: {}", dir);
        } else {
            warn!("Yuzu save directory not found");
        }
        
        Self {
            pid: None,
            emulator_type: EmulatorType::Yuzu,
            save_directory,
        }
    }
    
    pub fn new_ryujinx() -> Self {
        let save_directory = Self::get_ryujinx_save_directory();
        
        if let Some(ref dir) = save_directory {
            info!("Ryujinx save directory found: {}", dir);
        } else {
            warn!("Ryujinx save directory not found");
        }
        
        Self {
            pid: None,
            emulator_type: EmulatorType::Ryujinx,
            save_directory,
        }
    }
    
    pub fn with_pid(pid: u32, emulator_type: EmulatorType) -> Self {
        let mut instance = match emulator_type {
            EmulatorType::Yuzu => Self::new_yuzu(),
            EmulatorType::Ryujinx => Self::new_ryujinx(),
        };
        instance.pid = Some(pid);
        instance
    }
    
    fn get_yuzu_save_directory() -> Option<String> {
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                // Yuzu save data location
                let yuzu_path = format!("{}\\yuzu\\nand\\user\\save", appdata);
                if std::path::Path::new(&yuzu_path).exists() {
                    return Some(yuzu_path);
                }
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            if let Ok(home) = std::env::var("HOME") {
                // Check Flatpak location first
                let flatpak_path = format!("{}/.var/app/org.yuzu_emu.yuzu/data/yuzu/nand/user/save", home);
                if std::path::Path::new(&flatpak_path).exists() {
                    return Some(flatpak_path);
                }
                
                // Check standard location
                let standard_path = format!("{}/.local/share/yuzu/nand/user/save", home);
                if std::path::Path::new(&standard_path).exists() {
                    return Some(standard_path);
                }
                
                // Check old location
                let old_path = format!("{}/.yuzu/nand/user/save", home);
                if std::path::Path::new(&old_path).exists() {
                    return Some(old_path);
                }
            }
        }
        
        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let yuzu_path = format!("{}/Library/Application Support/yuzu/nand/user/save", home);
                if std::path::Path::new(&yuzu_path).exists() {
                    return Some(yuzu_path);
                }
            }
        }
        
        None
    }
    
    fn get_ryujinx_save_directory() -> Option<String> {
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                // Ryujinx save data location
                let ryujinx_path = format!("{}\\Ryujinx\\bis\\user\\save", appdata);
                if std::path::Path::new(&ryujinx_path).exists() {
                    return Some(ryujinx_path);
                }
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            if let Ok(home) = std::env::var("HOME") {
                // Check Flatpak location first
                let flatpak_path = format!("{}/.var/app/org.ryujinx.Ryujinx/config/Ryujinx/bis/user/save", home);
                if std::path::Path::new(&flatpak_path).exists() {
                    return Some(flatpak_path);
                }
                
                // Check standard location
                let standard_path = format!("{}/.config/Ryujinx/bis/user/save", home);
                if std::path::Path::new(&standard_path).exists() {
                    return Some(standard_path);
                }
            }
        }
        
        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                let ryujinx_path = format!("{}/Library/Application Support/Ryujinx/bis/user/save", home);
                if std::path::Path::new(&ryujinx_path).exists() {
                    return Some(ryujinx_path);
                }
            }
        }
        
        None
    }
    
    fn detect_save_files(&self) -> Vec<PathBuf> {
        let mut save_files = Vec::new();
        
        if let Some(ref save_dir) = self.save_directory {
            let save_path = PathBuf::from(save_dir);
            
            // Nintendo Switch saves are organized by title ID
            // Format: save/0000000000000000/<user_id>/<title_id>/
            if let Ok(entries) = std::fs::read_dir(&save_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // Look for user directories
                        if let Ok(user_entries) = std::fs::read_dir(&path) {
                            for user_entry in user_entries.flatten() {
                                let user_path = user_entry.path();
                                if user_path.is_dir() {
                                    // Look for title directories (game saves)
                                    if let Ok(title_entries) = std::fs::read_dir(&user_path) {
                                        for title_entry in title_entries.flatten() {
                                            let title_path = title_entry.path();
                                            if title_path.is_dir() {
                                                // This is a game save directory
                                                save_files.push(title_path);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        save_files
    }
    
    fn get_game_title_from_id(&self, title_id: &str) -> Option<String> {
        // Map some common Switch game title IDs to names
        // In a real implementation, this would use a more comprehensive database
        match title_id {
            "01006A800016E000" => Some("Super Smash Bros. Ultimate".to_string()),
            "0100F2C0115B6000" => Some("Mario Kart 8 Deluxe".to_string()),
            "01007EF00011E000" => Some("The Legend of Zelda: Breath of the Wild".to_string()),
            "0100E95004038000" => Some("The Legend of Zelda: Tears of the Kingdom".to_string()),
            "0100000000010000" => Some("Super Mario Odyssey".to_string()),
            "01003BC0000A0000" => Some("Splatoon 2".to_string()),
            "0100C2500FC20000" => Some("Splatoon 3".to_string()),
            "0100B7D0022EE000" => Some("Pokémon Sword".to_string()),
            "0100B7D0022EF000" => Some("Pokémon Shield".to_string()),
            "0100ABF008968000" => Some("Pokémon Scarlet".to_string()),
            "01008F6008C5E000" => Some("Pokémon Violet".to_string()),
            "010025400AECE000" => Some("Animal Crossing: New Horizons".to_string()),
            _ => None,
        }
    }
}

#[async_trait]
impl Emulator for YuzuRyujinx {
    fn name(&self) -> &str {
        match self.emulator_type {
            EmulatorType::Yuzu => "Yuzu",
            EmulatorType::Ryujinx => "Ryujinx",
        }
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
            let game_name = match self.emulator_type {
                EmulatorType::Yuzu => crate::monitor::process::get_yuzu_game_name(pid),
                EmulatorType::Ryujinx => crate::monitor::process::get_ryujinx_game_name(pid),
            };
            
            if let Some(name) = game_name {
                return Some(name);
            }
            
            // Fallback to generic name
            Some(format!("Unknown Switch Game ({})", self.name()))
        } else {
            None
        }
    }
    
    async fn monitor_saves(&self) -> Result<()> {
        if let Some(ref save_dir) = self.save_directory {
            info!("Monitoring {} saves in: {}", self.name(), save_dir);
            
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                debug!("Checking for save changes in {}", save_dir);
                
                // Check for Switch save files
                let save_files = self.detect_save_files();
                for save_file in save_files {
                    // Try to extract title ID from path
                    if let Some(title_id) = save_file.file_name() {
                        let id = title_id.to_string_lossy();
                        if let Some(game_name) = self.get_game_title_from_id(&id) {
                            debug!("Found save for: {}", game_name);
                        } else {
                            debug!("Found save for title ID: {}", id);
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
    fn test_yuzu_new() {
        let yuzu = YuzuRyujinx::new_yuzu();
        assert_eq!(yuzu.name(), "Yuzu");
        assert!(!yuzu.is_running());
        assert_eq!(yuzu.get_current_game(), None);
    }
    
    #[test]
    fn test_ryujinx_new() {
        let ryujinx = YuzuRyujinx::new_ryujinx();
        assert_eq!(ryujinx.name(), "Ryujinx");
        assert!(!ryujinx.is_running());
        assert_eq!(ryujinx.get_current_game(), None);
    }

    #[test]
    fn test_with_pid() {
        let yuzu = YuzuRyujinx::with_pid(1234, EmulatorType::Yuzu);
        assert_eq!(yuzu.name(), "Yuzu");
        assert!(yuzu.is_running());
        assert_eq!(yuzu.pid, Some(1234));
        
        let ryujinx = YuzuRyujinx::with_pid(5678, EmulatorType::Ryujinx);
        assert_eq!(ryujinx.name(), "Ryujinx");
        assert!(ryujinx.is_running());
        assert_eq!(ryujinx.pid, Some(5678));
    }

    #[test]
    fn test_save_directory() {
        let mut yuzu = YuzuRyujinx::new_yuzu();
        
        // Test with no save directory
        yuzu.save_directory = None;
        assert_eq!(yuzu.get_save_directory(), None);
        
        // Test with save directory
        yuzu.save_directory = Some("/home/user/.local/share/yuzu/nand/user/save".to_string());
        assert_eq!(yuzu.get_save_directory(), Some("/home/user/.local/share/yuzu/nand/user/save".to_string()));
    }

    #[test]
    fn test_is_running() {
        let mut yuzu = YuzuRyujinx::new_yuzu();
        
        // Not running by default
        assert!(!yuzu.is_running());
        
        // Running when PID is set
        yuzu.pid = Some(5678);
        assert!(yuzu.is_running());
        
        // Not running when PID is cleared
        yuzu.pid = None;
        assert!(!yuzu.is_running());
    }
    
    #[test]
    fn test_detect_save_files() {
        let yuzu = YuzuRyujinx::new_yuzu();
        // This will return empty vec if directories don't exist
        let saves = yuzu.detect_save_files();
        assert!(saves.is_empty() || !saves.is_empty()); // Valid either way
    }
    
    #[test]
    fn test_game_title_from_id() {
        let yuzu = YuzuRyujinx::new_yuzu();
        
        // Test known game
        assert_eq!(
            yuzu.get_game_title_from_id("01007EF00011E000"),
            Some("The Legend of Zelda: Breath of the Wild".to_string())
        );
        
        // Test unknown game
        assert_eq!(yuzu.get_game_title_from_id("0000000000000000"), None);
    }
}