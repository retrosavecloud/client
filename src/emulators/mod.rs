pub mod pcsx2;

use async_trait::async_trait;
use anyhow::Result;

/// Trait that all emulator implementations must follow
#[async_trait]
pub trait Emulator {
    /// Get the name of the emulator
    fn name(&self) -> &str;
    
    /// Get the save file directory for this emulator
    fn get_save_directory(&self) -> Option<String>;
    
    /// Detect if the emulator is currently running
    fn is_running(&self) -> bool;
    
    /// Get the currently running game (if detectable)
    fn get_current_game(&self) -> Option<String>;
    
    /// Monitor save file changes
    async fn monitor_saves(&self) -> Result<()>;
}