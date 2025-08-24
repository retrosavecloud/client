use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use tracing::{info, debug, warn};
use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::Reader;

/// EmulationStation integration for retro gaming frontends
pub struct EmulationStation {
    config_path: Option<PathBuf>,
    systems: Vec<System>,
    collections: Vec<Collection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct System {
    pub name: String,
    pub fullname: String,
    pub path: String,
    pub extension: Vec<String>,
    pub command: String,
    pub platform: String,
    pub theme: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub name: String,
    pub games: Vec<GameListEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameListEntry {
    pub path: String,
    pub name: String,
    pub desc: Option<String>,
    pub image: Option<String>,
    pub rating: Option<f32>,
    pub releasedate: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub genre: Option<String>,
    pub players: Option<String>,
    pub lastplayed: Option<String>,
    pub playcount: Option<u32>,
    pub favorite: Option<bool>,
    pub hidden: Option<bool>,
}

impl EmulationStation {
    pub fn new() -> Self {
        let config_path = Self::find_config_directory();
        
        let mut es = Self {
            config_path: config_path.clone(),
            systems: Vec::new(),
            collections: Vec::new(),
        };
        
        if config_path.is_some() {
            if let Err(e) = es.load_configuration() {
                warn!("Failed to load EmulationStation configuration: {}", e);
            }
        }
        
        es
    }
    
    /// Find EmulationStation configuration directory
    fn find_config_directory() -> Option<PathBuf> {
        let possible_paths = vec![
            // Linux standard locations
            #[cfg(target_os = "linux")]
            Self::get_home_path().map(|p| p.join(".emulationstation")).unwrap_or_default(),
            #[cfg(target_os = "linux")]
            Self::get_home_path().map(|p| p.join(".config/emulationstation")).unwrap_or_default(),
            #[cfg(target_os = "linux")]
            PathBuf::from("/etc/emulationstation"),
            
            // RetroPie location
            #[cfg(target_os = "linux")]
            Self::get_home_path().map(|p| p.join("RetroPie/configs/all/emulationstation")).unwrap_or_default(),
            #[cfg(target_os = "linux")]
            PathBuf::from("/opt/retropie/configs/all/emulationstation"),
            
            // Batocera location
            #[cfg(target_os = "linux")]
            PathBuf::from("/userdata/system/configs/emulationstation"),
            
            // Windows locations
            #[cfg(target_os = "windows")]
            Self::get_home_path().map(|p| p.join(".emulationstation")).unwrap_or_default(),
            #[cfg(target_os = "windows")]
            Self::get_appdata_path().map(|p| p.join("EmulationStation")).unwrap_or_default(),
            
            // macOS locations
            #[cfg(target_os = "macos")]
            Self::get_home_path().map(|p| p.join(".emulationstation")).unwrap_or_default(),
            #[cfg(target_os = "macos")]
            Self::get_home_path().map(|p| p.join("Library/Application Support/EmulationStation")).unwrap_or_default(),
            
            // Portable installation
            PathBuf::from("./emulationstation"),
        ];
        
        for path in possible_paths {
            if path.exists() && path.join("es_systems.cfg").exists() {
                info!("Found EmulationStation configuration at: {}", path.display());
                return Some(path);
            }
        }
        
        // Check environment variable
        if let Ok(es_home) = std::env::var("ES_HOME") {
            let path = PathBuf::from(es_home);
            if path.exists() {
                info!("Found EmulationStation via ES_HOME at: {}", path.display());
                return Some(path);
            }
        }
        
        debug!("EmulationStation configuration not found");
        None
    }
    
    fn get_home_path() -> Option<PathBuf> {
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(PathBuf::from)
    }
    
    #[cfg(target_os = "windows")]
    fn get_appdata_path() -> Option<PathBuf> {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    }
    
    #[cfg(not(target_os = "windows"))]
    fn get_appdata_path() -> Option<PathBuf> {
        None
    }
    
    /// Load EmulationStation configuration
    fn load_configuration(&mut self) -> Result<()> {
        if let Some(config_path) = self.config_path.clone() {
            // Load systems configuration
            let systems_file = config_path.join("es_systems.cfg");
            if systems_file.exists() {
                self.load_systems(&systems_file)?;
            }
            
            // Load custom collections
            let collections_dir = config_path.join("collections");
            if collections_dir.exists() {
                self.load_collections(&collections_dir)?;
            }
            
            // Load gamelists for each system
            let gamelists_dir = config_path.join("gamelists");
            if gamelists_dir.exists() {
                self.load_gamelists(&gamelists_dir)?;
            }
        }
        
        Ok(())
    }
    
    /// Load systems configuration from es_systems.cfg
    fn load_systems(&mut self, path: &Path) -> Result<()> {
        let content = fs::read_to_string(path)?;
        let mut reader = Reader::from_str(&content);
        reader.config_mut().trim_text(true);
        
        let mut buf = Vec::new();
        let mut current_system: Option<System> = None;
        let mut current_element = String::new();
        
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    current_element = std::str::from_utf8(e.name().as_ref())
                        .unwrap_or("")
                        .to_string();
                    
                    if current_element == "system" {
                        current_system = Some(System {
                            name: String::new(),
                            fullname: String::new(),
                            path: String::new(),
                            extension: Vec::new(),
                            command: String::new(),
                            platform: String::new(),
                            theme: None,
                        });
                    }
                }
                Ok(Event::Text(e)) => {
                    if let Some(ref mut system) = current_system {
                        let text = e.unescape()?.to_string();
                        match current_element.as_str() {
                            "name" => system.name = text,
                            "fullname" => system.fullname = text,
                            "path" => system.path = text,
                            "extension" => {
                                system.extension = text.split_whitespace()
                                    .map(|s| s.to_string())
                                    .collect();
                            }
                            "command" => system.command = text,
                            "platform" => system.platform = text,
                            "theme" => system.theme = Some(text),
                            _ => {}
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let elem_name = std::str::from_utf8(e.name().as_ref())
                        .unwrap_or("")
                        .to_string();
                    if elem_name == "system" {
                        if let Some(system) = current_system.take() {
                            self.systems.push(system);
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    warn!("Error parsing es_systems.cfg: {}", e);
                    break;
                }
                _ => {}
            }
            buf.clear();
        }
        
        info!("Loaded {} systems from EmulationStation", self.systems.len());
        Ok(())
    }
    
    /// Load custom collections
    fn load_collections(&mut self, collections_dir: &Path) -> Result<()> {
        for entry in fs::read_dir(collections_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("cfg") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    debug!("Loading collection: {}", name);
                    
                    let content = fs::read_to_string(&path)?;
                    let games: Vec<GameListEntry> = content
                        .lines()
                        .map(|line| GameListEntry {
                            path: line.to_string(),
                            name: line.to_string(),
                            desc: None,
                            image: None,
                            rating: None,
                            releasedate: None,
                            developer: None,
                            publisher: None,
                            genre: None,
                            players: None,
                            lastplayed: None,
                            playcount: None,
                            favorite: None,
                            hidden: None,
                        })
                        .collect();
                    
                    self.collections.push(Collection {
                        name: name.to_string(),
                        games,
                    });
                }
            }
        }
        
        info!("Loaded {} collections", self.collections.len());
        Ok(())
    }
    
    /// Load gamelists for systems
    fn load_gamelists(&mut self, gamelists_dir: &Path) -> Result<()> {
        for system in &mut self.systems {
            let gamelist_file = gamelists_dir.join(&system.name).join("gamelist.xml");
            if gamelist_file.exists() {
                debug!("Loading gamelist for {}", system.name);
                // Would parse gamelist.xml here
                // For now, we skip the actual parsing
            }
        }
        
        Ok(())
    }
    
    /// Get all configured systems
    pub fn get_systems(&self) -> &[System] {
        &self.systems
    }
    
    /// Find a system by name
    pub fn find_system(&self, name: &str) -> Option<&System> {
        self.systems.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }
    
    /// Get ROM directory for a system
    pub fn get_rom_directory(&self, system_name: &str) -> Option<String> {
        self.find_system(system_name).map(|s| s.path.clone())
    }
    
    /// Get emulator command for a system
    pub fn get_emulator_command(&self, system_name: &str) -> Option<String> {
        self.find_system(system_name).map(|s| s.command.clone())
    }
    
    /// Check if EmulationStation is configured
    pub fn is_configured(&self) -> bool {
        self.config_path.is_some()
    }
    
    /// Get configuration path
    pub fn get_config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }
    
    /// Get collections
    pub fn get_collections(&self) -> &[Collection] {
        &self.collections
    }
    
    /// Find collection by name
    pub fn find_collection(&self, name: &str) -> Option<&Collection> {
        self.collections.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }
    
    /// Get recently played games
    pub fn get_recent_games(&self, limit: usize) -> Vec<GameListEntry> {
        let mut recent: Vec<GameListEntry> = Vec::new();
        
        for collection in &self.collections {
            for game in &collection.games {
                if game.lastplayed.is_some() {
                    recent.push(game.clone());
                }
            }
        }
        
        // Sort by last played (newest first)
        recent.sort_by(|a, b| b.lastplayed.cmp(&a.lastplayed));
        recent.truncate(limit);
        
        recent
    }
    
    /// Get favorite games
    pub fn get_favorites(&self) -> Vec<GameListEntry> {
        let mut favorites = Vec::new();
        
        for collection in &self.collections {
            for game in &collection.games {
                if game.favorite.unwrap_or(false) {
                    favorites.push(game.clone());
                }
            }
        }
        
        favorites
    }
    
    /// Map system to emulator
    pub fn map_system_to_emulator(&self, system_name: &str) -> Option<String> {
        // Map EmulationStation systems to our emulator names
        let mapping = match system_name.to_lowercase().as_str() {
            "psx" | "ps1" | "playstation" => Some("PCSX2"),
            "ps2" | "playstation2" => Some("PCSX2"),
            "gamecube" | "gc" => Some("Dolphin"),
            "wii" => Some("Dolphin"),
            "ps3" | "playstation3" => Some("RPCS3"),
            "3ds" => Some("Citra"),
            "psp" => Some("PPSSPP"),
            "switch" => Some("Yuzu"),
            "nes" | "snes" | "gb" | "gba" | "gbc" | "genesis" | "megadrive" => Some("RetroArch"),
            _ => None,
        };
        
        mapping.map(|s| s.to_string())
    }
    
    /// Watch for game launches from EmulationStation
    pub async fn watch_for_launches(&self) -> Result<()> {
        if !self.is_configured() {
            return Err(anyhow::anyhow!("EmulationStation is not configured"));
        }
        
        info!("Watching for EmulationStation game launches");
        
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            
            // Check if EmulationStation is running
            if Self::is_emulationstation_running() {
                debug!("EmulationStation is running");
                
                // Monitor for launched emulators
                // This would integrate with the process monitoring
            }
        }
    }
    
    /// Check if EmulationStation is running
    fn is_emulationstation_running() -> bool {
        use sysinfo::{System, ProcessesToUpdate};
        
        let mut system = System::new_all();
        system.refresh_processes(ProcessesToUpdate::All, true);
        
        for (_pid, process) in system.processes() {
            let process_name = process.name().to_string_lossy().to_lowercase();
            if process_name.contains("emulationstation") {
                return true;
            }
        }
        
        false
    }
    
    /// Export systems to JSON
    pub fn export_systems_json(&self) -> Result<String> {
        let json = serde_json::to_string_pretty(&self.systems)?;
        Ok(json)
    }
}

impl Default for EmulationStation {
    fn default() -> Self {
        Self::new()
    }
}

/// RetroPie specific extensions
pub struct RetroPie {
    base_path: PathBuf,
}

impl RetroPie {
    pub fn new() -> Option<Self> {
        let possible_paths = vec![
            PathBuf::from("/home/pi/RetroPie"),
            PathBuf::from("/opt/retropie"),
            Self::get_home_path()?.join("RetroPie"),
        ];
        
        for path in possible_paths {
            if path.exists() {
                info!("Found RetroPie installation at: {}", path.display());
                return Some(Self { base_path: path });
            }
        }
        
        None
    }
    
    fn get_home_path() -> Option<PathBuf> {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
    
    pub fn get_roms_path(&self) -> PathBuf {
        self.base_path.join("roms")
    }
    
    pub fn get_bios_path(&self) -> PathBuf {
        self.base_path.join("BIOS")
    }
    
    pub fn get_configs_path(&self) -> PathBuf {
        self.base_path.join("configs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emulationstation_new() {
        let es = EmulationStation::new();
        // Will only find config if EmulationStation is installed
        if es.is_configured() {
            assert!(es.get_config_path().is_some());
        }
    }

    #[test]
    fn test_find_system() {
        let mut es = EmulationStation::new();
        
        // Add test system
        es.systems.push(System {
            name: "snes".to_string(),
            fullname: "Super Nintendo".to_string(),
            path: "/home/pi/RetroPie/roms/snes".to_string(),
            extension: vec!["smc".to_string(), "sfc".to_string()],
            command: "retroarch".to_string(),
            platform: "snes".to_string(),
            theme: None,
        });
        
        assert!(es.find_system("snes").is_some());
        assert!(es.find_system("SNES").is_some()); // Case insensitive
        assert!(es.find_system("nonexistent").is_none());
    }

    #[test]
    fn test_map_system_to_emulator() {
        let es = EmulationStation::new();
        
        assert_eq!(es.map_system_to_emulator("ps2"), Some("PCSX2".to_string()));
        assert_eq!(es.map_system_to_emulator("gamecube"), Some("Dolphin".to_string()));
        assert_eq!(es.map_system_to_emulator("psp"), Some("PPSSPP".to_string()));
        assert_eq!(es.map_system_to_emulator("snes"), Some("RetroArch".to_string()));
        assert_eq!(es.map_system_to_emulator("unknown"), None);
    }

    #[test]
    fn test_get_rom_directory() {
        let mut es = EmulationStation::new();
        
        es.systems.push(System {
            name: "nes".to_string(),
            fullname: "Nintendo Entertainment System".to_string(),
            path: "/home/pi/RetroPie/roms/nes".to_string(),
            extension: vec!["nes".to_string(), "zip".to_string()],
            command: "retroarch".to_string(),
            platform: "nes".to_string(),
            theme: None,
        });
        
        assert_eq!(es.get_rom_directory("nes"), Some("/home/pi/RetroPie/roms/nes".to_string()));
        assert_eq!(es.get_rom_directory("nonexistent"), None);
    }

    #[test]
    fn test_retropie_detection() {
        let _retropie = RetroPie::new();
        // Can't assert much as it depends on system
    }

    #[test]
    fn test_export_systems_json() {
        let mut es = EmulationStation::new();
        
        es.systems.push(System {
            name: "test".to_string(),
            fullname: "Test System".to_string(),
            path: "/test/path".to_string(),
            extension: vec!["tst".to_string()],
            command: "test_emu".to_string(),
            platform: "test".to_string(),
            theme: None,
        });
        
        let json = es.export_systems_json().unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("Test System"));
    }
}