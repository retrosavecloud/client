pub mod launchbox;
pub mod emulationstation;

use async_trait::async_trait;
use anyhow::Result;
use std::path::Path;

/// Trait for frontend launcher integrations
#[async_trait]
pub trait Launcher {
    /// Get the name of the launcher
    fn name(&self) -> &str;
    
    /// Check if the launcher is installed
    fn is_installed(&self) -> bool;
    
    /// Get installation path
    fn get_install_path(&self) -> Option<&Path>;
    
    /// Get list of configured emulators
    fn get_emulators(&self) -> Vec<String>;
    
    /// Get list of games
    fn get_games(&self) -> Vec<GameInfo>;
    
    /// Watch for game launches
    async fn watch_launches(&self) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct GameInfo {
    pub id: String,
    pub title: String,
    pub platform: String,
    pub emulator: String,
    pub rom_path: String,
    pub save_path: Option<String>,
    pub last_played: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LauncherEvent {
    pub launcher: String,
    pub event_type: LauncherEventType,
    pub game: Option<GameInfo>,
}

#[derive(Debug, Clone)]
pub enum LauncherEventType {
    GameLaunched,
    GameClosed,
    LauncherStarted,
    LauncherStopped,
}

/// Manager for multiple launcher integrations
pub struct LauncherManager {
    launchers: Vec<Box<dyn Launcher + Send + Sync>>,
}

impl LauncherManager {
    pub fn new() -> Self {
        let launchers: Vec<Box<dyn Launcher + Send + Sync>> = Vec::new();
        
        // Add LaunchBox support
        let launchbox = launchbox::LaunchBox::new();
        if launchbox.is_installed() {
            // Note: LaunchBox doesn't implement Launcher trait yet
            // We'd need to impl Launcher for LaunchBox
        }
        
        Self { launchers }
    }
    
    /// Get all installed launchers
    pub fn get_installed_launchers(&self) -> Vec<String> {
        self.launchers
            .iter()
            .filter(|l| l.is_installed())
            .map(|l| l.name().to_string())
            .collect()
    }
    
    /// Find a game across all launchers
    pub fn find_game(&self, title: &str) -> Option<GameInfo> {
        for launcher in &self.launchers {
            let games = launcher.get_games();
            if let Some(game) = games.iter().find(|g| g.title.eq_ignore_ascii_case(title)) {
                return Some(game.clone());
            }
        }
        None
    }
}

impl Default for LauncherManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launcher_manager_new() {
        let manager = LauncherManager::new();
        // Will have launchers only if they're installed
        let _installed = manager.get_installed_launchers();
    }
}