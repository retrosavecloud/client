use anyhow::Result;
use crate::ui::settings::Settings;
use crate::storage::Database;
use std::sync::Arc;
use tracing::{info, debug};

/// Manager for persisting and loading application settings
pub struct SettingsManager {
    db: Arc<Database>,
}

impl SettingsManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
    
    /// Load settings from database, returns default if not found
    pub async fn load_settings(&self) -> Result<Settings> {
        let mut settings = Settings::default();
        
        // Load each setting from database
        if let Some(value) = self.db.get_setting("auto_save_enabled").await? {
            settings.auto_save_enabled = value == "true";
        }
        
        if let Some(value) = self.db.get_setting("save_interval_minutes").await? {
            if let Ok(minutes) = value.parse::<u32>() {
                settings.save_interval_minutes = minutes;
            }
        }
        
        if let Some(value) = self.db.get_setting("max_saves_per_game").await? {
            if let Ok(max) = value.parse::<u32>() {
                settings.max_saves_per_game = max;
            }
        }
        
        if let Some(value) = self.db.get_setting("start_on_boot").await? {
            settings.start_on_boot = value == "true";
        }
        
        if let Some(value) = self.db.get_setting("minimize_to_tray").await? {
            settings.minimize_to_tray = value == "true";
        }
        
        if let Some(value) = self.db.get_setting("show_notifications").await? {
            settings.show_notifications = value == "true";
        }
        
        if let Some(value) = self.db.get_setting("cloud_sync_enabled").await? {
            settings.cloud_sync_enabled = value == "true";
        }
        
        if let Some(value) = self.db.get_setting("hotkey_enabled").await? {
            settings.hotkey_enabled = value == "true";
        }
        
        if let Some(value) = self.db.get_setting("save_hotkey").await? {
            settings.save_hotkey = Some(value);
        }
        
        if let Some(value) = self.db.get_setting("compression_enabled").await? {
            settings.compression_enabled = value == "true";
        }
        
        if let Some(value) = self.db.get_setting("compression_level").await? {
            if let Ok(level) = value.parse::<i32>() {
                settings.compression_level = level.clamp(1, 22);
            }
        }
        
        // Always override API URL with the correct value based on environment
        // This ensures users cannot modify it even if they edited the database directly
        settings.update_api_url();
        
        debug!("Loaded settings from database");
        Ok(settings)
    }
    
    /// Save settings to database
    pub async fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.db.set_setting("auto_save_enabled", &settings.auto_save_enabled.to_string()).await?;
        self.db.set_setting("save_interval_minutes", &settings.save_interval_minutes.to_string()).await?;
        self.db.set_setting("max_saves_per_game", &settings.max_saves_per_game.to_string()).await?;
        self.db.set_setting("start_on_boot", &settings.start_on_boot.to_string()).await?;
        self.db.set_setting("minimize_to_tray", &settings.minimize_to_tray.to_string()).await?;
        self.db.set_setting("show_notifications", &settings.show_notifications.to_string()).await?;
        self.db.set_setting("cloud_sync_enabled", &settings.cloud_sync_enabled.to_string()).await?;
        self.db.set_setting("hotkey_enabled", &settings.hotkey_enabled.to_string()).await?;
        
        if let Some(ref hotkey) = settings.save_hotkey {
            self.db.set_setting("save_hotkey", hotkey).await?;
        }
        
        self.db.set_setting("compression_enabled", &settings.compression_enabled.to_string()).await?;
        self.db.set_setting("compression_level", &settings.compression_level.to_string()).await?;
        
        info!("Settings saved to database");
        Ok(())
    }
    
    /// Save a single setting
    pub async fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        self.db.set_setting(key, value).await?;
        debug!("Saved setting: {} = {}", key, value);
        Ok(())
    }
    
    /// Get a single setting
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.db.get_setting(key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_save_and_load_settings() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Arc::new(Database::new(Some(db_path)).await.unwrap());
        
        let manager = SettingsManager::new(db);
        
        // Create custom settings
        let mut settings = Settings::default();
        settings.auto_save_enabled = false;
        settings.save_interval_minutes = 10;
        settings.max_saves_per_game = 3;
        settings.start_on_boot = true;
        settings.save_hotkey = Some("Ctrl+Alt+S".to_string());
        
        // Save settings
        manager.save_settings(&settings).await.unwrap();
        
        // Load settings
        let loaded = manager.load_settings().await.unwrap();
        
        // Verify
        assert_eq!(loaded.auto_save_enabled, false);
        assert_eq!(loaded.save_interval_minutes, 10);
        assert_eq!(loaded.max_saves_per_game, 3);
        assert_eq!(loaded.start_on_boot, true);
        assert_eq!(loaded.save_hotkey, Some("Ctrl+Alt+S".to_string()));
    }
    
    #[tokio::test]
    async fn test_load_default_settings() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Arc::new(Database::new(Some(db_path)).await.unwrap());
        
        let manager = SettingsManager::new(db);
        
        // Load settings when none exist
        let loaded = manager.load_settings().await.unwrap();
        
        // Should get defaults
        assert_eq!(loaded.auto_save_enabled, true);
        assert_eq!(loaded.save_interval_minutes, 5);
        assert_eq!(loaded.max_saves_per_game, 5);
        assert_eq!(loaded.save_hotkey, Some("Ctrl+Shift+S".to_string()));
    }
}