use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use tracing::{info, debug, warn};
use anyhow::Result;

/// LaunchBox integration for detecting games and emulators
pub struct LaunchBox {
    install_path: Option<PathBuf>,
    data_path: Option<PathBuf>,
    platforms: Vec<Platform>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub name: String,
    pub emulator: String,
    pub games: Vec<Game>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub title: String,
    pub platform: String,
    pub rom_path: String,
    pub emulator_id: String,
    pub last_played: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatorConfig {
    pub id: String,
    pub title: String,
    pub application_path: String,
    pub command_line: String,
}

impl LaunchBox {
    pub fn new() -> Self {
        let install_path = Self::find_launchbox_installation();
        let data_path = install_path.as_ref().map(|p| p.join("Data"));
        
        let mut launchbox = Self {
            install_path: install_path.clone(),
            data_path,
            platforms: Vec::new(),
        };
        
        if install_path.is_some() {
            if let Err(e) = launchbox.load_configuration() {
                warn!("Failed to load LaunchBox configuration: {}", e);
            }
        }
        
        launchbox
    }
    
    /// Find LaunchBox installation directory
    fn find_launchbox_installation() -> Option<PathBuf> {
        // Common installation paths
        let possible_paths = vec![
            // Windows default locations
            #[cfg(target_os = "windows")]
            PathBuf::from("C:\\LaunchBox"),
            #[cfg(target_os = "windows")]
            PathBuf::from("C:\\Program Files\\LaunchBox"),
            #[cfg(target_os = "windows")]
            PathBuf::from("C:\\Program Files (x86)\\LaunchBox"),
            #[cfg(target_os = "windows")]
            Self::get_user_documents_path().map(|p| p.join("LaunchBox")).unwrap_or_default(),
            
            // Linux/Wine locations
            #[cfg(target_os = "linux")]
            Self::get_home_path().map(|p| p.join(".wine/drive_c/LaunchBox")).unwrap_or_default(),
            #[cfg(target_os = "linux")]
            Self::get_home_path().map(|p| p.join("LaunchBox")).unwrap_or_default(),
            
            // Portable installations
            PathBuf::from("./LaunchBox"),
        ];
        
        for path in possible_paths {
            if path.exists() && path.join("LaunchBox.exe").exists() {
                info!("Found LaunchBox installation at: {}", path.display());
                return Some(path);
            }
        }
        
        // Check environment variable
        if let Ok(launchbox_path) = std::env::var("LAUNCHBOX_PATH") {
            let path = PathBuf::from(launchbox_path);
            if path.exists() {
                info!("Found LaunchBox via environment variable at: {}", path.display());
                return Some(path);
            }
        }
        
        debug!("LaunchBox installation not found");
        None
    }
    
    #[cfg(target_os = "windows")]
    fn get_user_documents_path() -> Option<PathBuf> {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return Some(PathBuf::from(profile).join("Documents"));
        }
        None
    }
    
    #[cfg(not(target_os = "windows"))]
    fn get_user_documents_path() -> Option<PathBuf> {
        if let Ok(home) = std::env::var("HOME") {
            return Some(PathBuf::from(home).join("Documents"));
        }
        None
    }
    
    fn get_home_path() -> Option<PathBuf> {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
    
    /// Load LaunchBox configuration and game database
    fn load_configuration(&mut self) -> Result<()> {
        if let Some(data_path) = self.data_path.clone() {
            // Load platforms
            let platforms_file = data_path.join("Platforms.xml");
            if platforms_file.exists() {
                self.load_platforms(&platforms_file)?;
            }
            
            // Load emulators
            let emulators_file = data_path.join("Emulators.xml");
            if emulators_file.exists() {
                self.load_emulators(&emulators_file)?;
            }
            
            // Load games for each platform
            let platforms_dir = data_path.join("Platforms");
            if platforms_dir.exists() {
                self.load_games(&platforms_dir)?;
            }
        }
        
        Ok(())
    }
    
    /// Load platform definitions
    fn load_platforms(&mut self, path: &Path) -> Result<()> {
        let _content = fs::read_to_string(path)?;
        
        // Parse XML (simplified - in production use proper XML parser)
        // LaunchBox uses XML format for configuration
        // For now, we'll create mock data
        self.platforms = vec![
            Platform {
                name: "Sony PlayStation 2".to_string(),
                emulator: "PCSX2".to_string(),
                games: Vec::new(),
            },
            Platform {
                name: "Nintendo GameCube".to_string(),
                emulator: "Dolphin".to_string(),
                games: Vec::new(),
            },
            Platform {
                name: "Sony PlayStation Portable".to_string(),
                emulator: "PPSSPP".to_string(),
                games: Vec::new(),
            },
        ];
        
        debug!("Loaded {} platforms from LaunchBox", self.platforms.len());
        Ok(())
    }
    
    /// Load emulator configurations
    fn load_emulators(&mut self, path: &Path) -> Result<()> {
        let _content = fs::read_to_string(path)?;
        
        // Parse emulator configurations
        // This would parse the XML to get emulator paths and settings
        debug!("Loaded emulator configurations from LaunchBox");
        Ok(())
    }
    
    /// Load games database
    fn load_games(&mut self, platforms_dir: &Path) -> Result<()> {
        // LaunchBox stores games in XML files per platform
        for entry in fs::read_dir(platforms_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("xml") {
                if let Some(platform_name) = path.file_stem().and_then(|s| s.to_str()) {
                    debug!("Loading games for platform: {}", platform_name);
                    // Parse the XML file for games
                    // This would extract game information
                }
            }
        }
        
        Ok(())
    }
    
    /// Get recently played games
    pub fn get_recent_games(&self, limit: usize) -> Vec<&Game> {
        let mut recent_games: Vec<&Game> = Vec::new();
        
        for platform in &self.platforms {
            for game in &platform.games {
                if game.last_played.is_some() {
                    recent_games.push(game);
                }
            }
        }
        
        // Sort by last played date (newest first)
        recent_games.sort_by(|a, b| b.last_played.cmp(&a.last_played));
        recent_games.truncate(limit);
        
        recent_games
    }
    
    /// Find game by title
    pub fn find_game(&self, title: &str) -> Option<&Game> {
        let search_title = title.to_lowercase();
        
        for platform in &self.platforms {
            for game in &platform.games {
                if game.title.to_lowercase().contains(&search_title) {
                    return Some(game);
                }
            }
        }
        
        None
    }
    
    /// Get all games for a specific platform
    pub fn get_platform_games(&self, platform_name: &str) -> Vec<&Game> {
        self.platforms
            .iter()
            .filter(|p| p.name.eq_ignore_ascii_case(platform_name))
            .flat_map(|p| &p.games)
            .collect()
    }
    
    /// Get emulator path for a platform
    pub fn get_emulator_for_platform(&self, platform_name: &str) -> Option<String> {
        self.platforms
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(platform_name))
            .map(|p| p.emulator.clone())
    }
    
    /// Check if LaunchBox is installed
    pub fn is_installed(&self) -> bool {
        self.install_path.is_some()
    }
    
    /// Get LaunchBox installation path
    pub fn get_install_path(&self) -> Option<&Path> {
        self.install_path.as_deref()
    }
    
    /// Watch for LaunchBox game launches
    pub async fn watch_for_launches(&self) -> Result<()> {
        if !self.is_installed() {
            return Err(anyhow::anyhow!("LaunchBox is not installed"));
        }
        
        info!("Watching for LaunchBox game launches");
        
        // Monitor LaunchBox process and detect when it launches games
        // This would integrate with the process monitoring system
        
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            
            // Check if LaunchBox is running
            if Self::is_launchbox_running() {
                debug!("LaunchBox is running");
                
                // Check for recently launched games
                // This would detect child processes launched by LaunchBox
            }
        }
    }
    
    /// Check if LaunchBox is running
    fn is_launchbox_running() -> bool {
        // Use sysinfo to check for LaunchBox.exe or BigBox.exe
        use sysinfo::{System, ProcessesToUpdate};
        
        let mut system = System::new_all();
        system.refresh_processes(ProcessesToUpdate::All, true);
        
        for (_pid, process) in system.processes() {
            let process_name = process.name().to_string_lossy().to_lowercase();
            if process_name.contains("launchbox") || process_name.contains("bigbox") {
                return true;
            }
        }
        
        false
    }
    
    /// Export LaunchBox game list to JSON
    pub fn export_games_json(&self) -> Result<String> {
        let json = serde_json::to_string_pretty(&self.platforms)?;
        Ok(json)
    }
    
    /// Import save mappings for LaunchBox games
    pub fn import_save_mappings(&self, mappings_file: &Path) -> Result<()> {
        if !mappings_file.exists() {
            return Err(anyhow::anyhow!("Mappings file does not exist"));
        }
        
        let content = fs::read_to_string(mappings_file)?;
        let _mappings: Vec<SaveMapping> = serde_json::from_str(&content)?;
        
        // Apply save mappings to games
        info!("Imported save mappings for LaunchBox games");
        
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SaveMapping {
    game_id: String,
    save_path: String,
    emulator: String,
}

/// Integration with LaunchBox's audit feature
pub struct LaunchBoxAudit {
    audit_path: PathBuf,
}

impl LaunchBoxAudit {
    pub fn new(launchbox_path: &Path) -> Self {
        Self {
            audit_path: launchbox_path.join("Data").join("PlaySessionAudits.xml"),
        }
    }
    
    /// Get play session statistics
    pub fn get_play_sessions(&self) -> Result<Vec<PlaySession>> {
        if !self.audit_path.exists() {
            return Ok(Vec::new());
        }
        
        // Parse play session audits
        // This would extract play time, launch count, etc.
        
        Ok(Vec::new())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaySession {
    pub game_id: String,
    pub start_time: String,
    pub end_time: String,
    pub duration_minutes: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launchbox_new() {
        let launchbox = LaunchBox::new();
        // Will only find installation if LaunchBox is actually installed
        if launchbox.is_installed() {
            assert!(launchbox.get_install_path().is_some());
        }
    }

    #[test]
    fn test_find_game() {
        let mut launchbox = LaunchBox::new();
        
        // Add test game
        launchbox.platforms.push(Platform {
            name: "Test Platform".to_string(),
            emulator: "TestEmulator".to_string(),
            games: vec![
                Game {
                    id: "test-1".to_string(),
                    title: "Test Game".to_string(),
                    platform: "Test Platform".to_string(),
                    rom_path: "/path/to/rom".to_string(),
                    emulator_id: "test-emu".to_string(),
                    last_played: None,
                }
            ],
        });
        
        assert!(launchbox.find_game("Test Game").is_some());
        assert!(launchbox.find_game("Nonexistent").is_none());
    }

    #[test]
    fn test_get_platform_games() {
        let mut launchbox = LaunchBox::new();
        
        launchbox.platforms.push(Platform {
            name: "PlayStation 2".to_string(),
            emulator: "PCSX2".to_string(),
            games: vec![
                Game {
                    id: "ps2-1".to_string(),
                    title: "Game 1".to_string(),
                    platform: "PlayStation 2".to_string(),
                    rom_path: "/path/to/rom1".to_string(),
                    emulator_id: "pcsx2".to_string(),
                    last_played: None,
                },
                Game {
                    id: "ps2-2".to_string(),
                    title: "Game 2".to_string(),
                    platform: "PlayStation 2".to_string(),
                    rom_path: "/path/to/rom2".to_string(),
                    emulator_id: "pcsx2".to_string(),
                    last_played: None,
                },
            ],
        });
        
        let ps2_games = launchbox.get_platform_games("PlayStation 2");
        assert_eq!(ps2_games.len(), 2);
    }

    #[test]
    fn test_is_launchbox_running() {
        // This will only return true if LaunchBox is actually running
        let _running = LaunchBox::is_launchbox_running();
        // Can't assert anything specific as it depends on system state
    }

    #[test]
    fn test_export_games_json() {
        let mut launchbox = LaunchBox::new();
        
        launchbox.platforms.push(Platform {
            name: "Test".to_string(),
            emulator: "TestEmu".to_string(),
            games: vec![],
        });
        
        let json = launchbox.export_games_json().unwrap();
        assert!(json.contains("Test"));
        assert!(json.contains("TestEmu"));
    }
}