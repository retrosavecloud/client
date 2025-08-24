pub mod pcsx2;
pub mod dolphin;

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

#[cfg(test)]
mod tests {
    use super::*;

    // Mock emulator for testing
    struct MockEmulator {
        name: String,
        save_dir: Option<String>,
        is_running: bool,
        current_game: Option<String>,
    }

    #[async_trait]
    impl Emulator for MockEmulator {
        fn name(&self) -> &str {
            &self.name
        }

        fn get_save_directory(&self) -> Option<String> {
            self.save_dir.clone()
        }

        fn is_running(&self) -> bool {
            self.is_running
        }

        fn get_current_game(&self) -> Option<String> {
            self.current_game.clone()
        }

        async fn monitor_saves(&self) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_emulator_trait_implementation() {
        let emulator = MockEmulator {
            name: "TestEmulator".to_string(),
            save_dir: Some("/path/to/saves".to_string()),
            is_running: true,
            current_game: Some("Test Game".to_string()),
        };

        assert_eq!(emulator.name(), "TestEmulator");
        assert_eq!(emulator.get_save_directory(), Some("/path/to/saves".to_string()));
        assert!(emulator.is_running());
        assert_eq!(emulator.get_current_game(), Some("Test Game".to_string()));
    }

    #[tokio::test]
    async fn test_monitor_saves() {
        let emulator = MockEmulator {
            name: "TestEmulator".to_string(),
            save_dir: None,
            is_running: false,
            current_game: None,
        };

        let result = emulator.monitor_saves().await;
        assert!(result.is_ok());
    }
}