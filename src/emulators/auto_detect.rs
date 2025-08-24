use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, debug, warn, error};
use crate::monitor::process::{self, EmulatorProcess};
use crate::storage::database::Database;
use crate::storage::watcher::{SaveWatcher, SaveEvent};

/// Manages multiple emulators simultaneously
pub struct EmulatorManager {
    database: Arc<Database>,
    tracked_emulators: HashMap<String, TrackedEmulator>,
    save_sender: mpsc::Sender<SaveEvent>,
}

struct TrackedEmulator {
    process: EmulatorProcess,
    save_watcher: Option<SaveWatcher>,
    save_receiver: Option<mpsc::Receiver<SaveEvent>>,
    current_game: Option<String>,
}

impl EmulatorManager {
    pub fn new(database: Arc<Database>, save_sender: mpsc::Sender<SaveEvent>) -> Self {
        Self {
            database,
            tracked_emulators: HashMap::new(),
            save_sender,
        }
    }
    
    /// Scan for all running emulators and update tracking
    pub async fn scan_emulators(&mut self) -> Vec<String> {
        let running_emulators = process::detect_running_emulators();
        let mut changes = Vec::new();
        
        // Track which emulators are currently running
        let mut currently_running = std::collections::HashSet::new();
        
        for emulator in running_emulators {
            let emulator_name = Self::get_emulator_name(&emulator).to_string();
            currently_running.insert(emulator_name.clone());
            
            // Check if this is a newly detected emulator
            if !self.tracked_emulators.contains_key(&emulator_name) {
                info!("Detected new emulator: {}", emulator_name);
                changes.push(format!("Started: {}", emulator_name));
                
                // Start tracking this emulator
                if let Err(e) = self.start_tracking_emulator(emulator).await {
                    error!("Failed to start tracking {}: {}", emulator_name, e);
                }
            }
        }
        
        // Check for stopped emulators
        let stopped: Vec<String> = self.tracked_emulators
            .keys()
            .filter(|name| !currently_running.contains(*name))
            .cloned()
            .collect();
        
        for emulator_name in stopped {
            info!("Emulator stopped: {}", emulator_name);
            changes.push(format!("Stopped: {}", emulator_name));
            self.stop_tracking_emulator(&emulator_name).await;
        }
        
        changes
    }
    
    /// Start tracking a specific emulator
    async fn start_tracking_emulator(&mut self, emulator: EmulatorProcess) -> anyhow::Result<()> {
        let emulator_name = Self::get_emulator_name(&emulator).to_string();
        
        // Get save directory for the emulator
        let save_dir = Self::get_save_directory(&emulator)?;
        info!("Setting up save monitoring for {} at: {}", emulator_name, save_dir.display());
        
        // Create save watcher
        let (mut watcher, receiver) = SaveWatcher::new(save_dir.clone(), self.database.clone())?;
        watcher.start().await?;
        
        // Get current game if possible
        let current_game = Self::detect_current_game(&emulator);
        
        let tracked = TrackedEmulator {
            process: emulator,
            save_watcher: Some(watcher),
            save_receiver: Some(receiver),
            current_game,
        };
        
        self.tracked_emulators.insert(emulator_name, tracked);
        
        Ok(())
    }
    
    /// Stop tracking an emulator
    async fn stop_tracking_emulator(&mut self, emulator_name: &str) {
        if let Some(mut tracked) = self.tracked_emulators.remove(emulator_name) {
            if let Some(mut watcher) = tracked.save_watcher.take() {
                watcher.stop();
                debug!("Stopped save watcher for {}", emulator_name);
            }
        }
    }
    
    /// Update game detection for all tracked emulators
    pub async fn update_games(&mut self) -> Vec<(String, String)> {
        let mut game_updates = Vec::new();
        
        for (emulator_name, tracked) in &mut self.tracked_emulators {
            if let Some(new_game) = Self::detect_current_game(&tracked.process) {
                if tracked.current_game.as_ref() != Some(&new_game) {
                    info!("{} now playing: {}", emulator_name, new_game);
                    game_updates.push((emulator_name.clone(), new_game.clone()));
                    tracked.current_game = Some(new_game.clone());
                    
                    // Update save watcher with new game
                    if let Some(ref mut watcher) = tracked.save_watcher {
                        watcher.set_current_game(Some(new_game)).await;
                    }
                }
            }
        }
        
        game_updates
    }
    
    /// Process save events from all watchers
    pub async fn process_save_events(&mut self) -> Vec<SaveEvent> {
        let mut events = Vec::new();
        
        for (emulator_name, tracked) in &mut self.tracked_emulators {
            if let Some(ref mut receiver) = tracked.save_receiver {
                // Try to receive without blocking
                match receiver.try_recv() {
                    Ok(mut event) => {
                        event.emulator = emulator_name.clone();
                        if let Some(ref game) = tracked.current_game {
                            debug!("Save event for {} playing {}", emulator_name, game);
                        }
                        events.push(event);
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {}
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        warn!("Save watcher channel disconnected for {}", emulator_name);
                        tracked.save_receiver = None;
                    }
                }
            }
        }
        
        events
    }
    
    /// Get list of currently tracked emulators
    pub fn get_tracked_emulators(&self) -> Vec<(String, Option<String>)> {
        self.tracked_emulators
            .iter()
            .map(|(name, tracked)| (name.clone(), tracked.current_game.clone()))
            .collect()
    }
    
    fn get_emulator_name(emulator: &EmulatorProcess) -> &str {
        match emulator {
            EmulatorProcess::PCSX2 { .. } => "PCSX2",
            EmulatorProcess::Dolphin { .. } => "Dolphin",
            EmulatorProcess::RPCS3 { .. } => "RPCS3",
            EmulatorProcess::Citra { .. } => "Citra",
            EmulatorProcess::RetroArch { .. } => "RetroArch",
            EmulatorProcess::Yuzu { .. } => "Yuzu",
            EmulatorProcess::Ryujinx { .. } => "Ryujinx",
            EmulatorProcess::PPSSPP { .. } => "PPSSPP",
        }
    }
    
    fn get_save_directory(emulator: &EmulatorProcess) -> anyhow::Result<PathBuf> {
        use crate::emulators::Emulator;
        
        let save_dir = match emulator {
            EmulatorProcess::PCSX2 { .. } => {
                let pcsx2 = crate::emulators::pcsx2::PCSX2::new();
                pcsx2.get_save_directory()
            }
            EmulatorProcess::Dolphin { .. } => {
                let dolphin = crate::emulators::dolphin::Dolphin::new();
                dolphin.get_save_directory()
            }
            EmulatorProcess::RPCS3 { .. } => {
                let rpcs3 = crate::emulators::rpcs3::RPCS3::new();
                rpcs3.get_save_directory()
            }
            EmulatorProcess::Citra { .. } => {
                let citra = crate::emulators::citra::Citra::new();
                citra.get_save_directory()
            }
            EmulatorProcess::RetroArch { .. } => {
                let retroarch = crate::emulators::retroarch::RetroArch::new();
                retroarch.get_save_directory()
            }
            EmulatorProcess::Yuzu { .. } => {
                let yuzu = crate::emulators::yuzu_ryujinx::YuzuRyujinx::new_yuzu();
                yuzu.get_save_directory()
            }
            EmulatorProcess::Ryujinx { .. } => {
                let ryujinx = crate::emulators::yuzu_ryujinx::YuzuRyujinx::new_ryujinx();
                ryujinx.get_save_directory()
            }
            EmulatorProcess::PPSSPP { .. } => {
                let ppsspp = crate::emulators::ppsspp::PPSSPP::new();
                ppsspp.get_save_directory()
            }
        };
        
        save_dir
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Could not find save directory"))
    }
    
    fn detect_current_game(emulator: &EmulatorProcess) -> Option<String> {
        match emulator {
            EmulatorProcess::PCSX2 { pid, .. } => process::get_pcsx2_game_name(*pid),
            EmulatorProcess::Dolphin { pid, .. } => process::get_dolphin_game_name(*pid),
            EmulatorProcess::RPCS3 { pid, .. } => process::get_rpcs3_game_name(*pid),
            EmulatorProcess::Citra { pid, .. } => process::get_citra_game_name(*pid),
            EmulatorProcess::RetroArch { pid, .. } => process::get_retroarch_game_name(*pid),
            EmulatorProcess::Yuzu { pid, .. } => process::get_yuzu_game_name(*pid),
            EmulatorProcess::Ryujinx { pid, .. } => process::get_ryujinx_game_name(*pid),
            EmulatorProcess::PPSSPP { pid, .. } => process::get_ppsspp_game_name(*pid),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_emulator_manager_new() {
        let database = Arc::new(Database::new(None).await.unwrap());
        let (tx, _rx) = mpsc::channel(100);
        
        let manager = EmulatorManager::new(database, tx);
        assert_eq!(manager.get_tracked_emulators().len(), 0);
    }

    #[tokio::test]
    async fn test_scan_with_no_emulators() {
        let database = Arc::new(Database::new(None).await.unwrap());
        let (tx, _rx) = mpsc::channel(100);
        
        let mut manager = EmulatorManager::new(database, tx);
        let changes = manager.scan_emulators().await;
        
        // Should detect no emulators when none are running
        assert_eq!(changes.len(), 0);
        assert_eq!(manager.get_tracked_emulators().len(), 0);
    }
}