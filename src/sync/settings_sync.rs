use anyhow::Result;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use tracing::{info, warn, error, debug};

use crate::ui::settings::Settings;
use super::api::SyncApi;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettingsResponse {
    // Save preferences
    pub auto_save_enabled: bool,
    pub save_interval_minutes: i32,
    pub max_saves_per_game: i32,
    
    // Notification preferences
    pub email_weekly_summary: bool,
    pub email_product_updates: bool,
    pub desktop_save_completed: bool,
    pub desktop_sync_errors: bool,
    
    // Storage preferences
    pub compression_enabled: bool,
    pub compression_level: i32,
    pub auto_cleanup_days: Option<i32>,
    
    // Metadata
    pub settings_version: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserSettings {
    // Save preferences
    pub auto_save_enabled: Option<bool>,
    pub save_interval_minutes: Option<i32>,
    pub max_saves_per_game: Option<i32>,
    
    // Notification preferences
    pub email_weekly_summary: Option<bool>,
    pub email_product_updates: Option<bool>,
    pub desktop_save_completed: Option<bool>,
    pub desktop_sync_errors: Option<bool>,
    
    // Storage preferences
    pub compression_enabled: Option<bool>,
    pub compression_level: Option<i32>,
    pub auto_cleanup_days: Option<Option<i32>>,
}

// Settings API methods are implemented in api.rs

/// Merge cloud settings with local settings
pub fn merge_settings(local: &Settings, cloud: UserSettingsResponse) -> Settings {
    Settings {
        // Cloud-synced settings
        auto_save_enabled: cloud.auto_save_enabled,
        save_interval_minutes: cloud.save_interval_minutes as u32,
        max_saves_per_game: cloud.max_saves_per_game as u32,
        compression_enabled: cloud.compression_enabled,
        compression_level: cloud.compression_level,
        show_notifications: cloud.desktop_save_completed || cloud.desktop_sync_errors,
        
        // Keep local-only settings unchanged
        start_on_boot: local.start_on_boot,
        minimize_to_tray: local.minimize_to_tray,
        cloud_sync_enabled: local.cloud_sync_enabled,
        cloud_api_url: local.cloud_api_url.clone(),
        cloud_auto_sync: local.cloud_auto_sync,
        hotkey_enabled: local.hotkey_enabled,
        save_hotkey: local.save_hotkey.clone(),
    }
}

/// Convert local settings to update request
pub fn settings_to_update(settings: &Settings) -> UpdateUserSettings {
    UpdateUserSettings {
        auto_save_enabled: Some(settings.auto_save_enabled),
        save_interval_minutes: Some(settings.save_interval_minutes as i32),
        max_saves_per_game: Some(settings.max_saves_per_game as i32),
        
        // Map notification settings
        email_weekly_summary: None, // Don't update email settings from desktop
        email_product_updates: None, // Don't update email settings from desktop
        desktop_save_completed: Some(settings.show_notifications),
        desktop_sync_errors: Some(settings.show_notifications),
        
        // Storage settings
        compression_enabled: Some(settings.compression_enabled),
        compression_level: Some(settings.compression_level),
        auto_cleanup_days: None, // Managed from web UI
    }
}

/// Sync settings from cloud on startup
pub async fn sync_settings_from_cloud(api: &SyncApi, local: &Settings) -> Result<Settings> {
    info!("Syncing settings from cloud...");
    
    match api.get_settings().await {
        Ok(cloud_settings) => {
            debug!("Received cloud settings: {:?}", cloud_settings);
            let merged = merge_settings(local, cloud_settings);
            info!("Settings synced from cloud successfully");
            Ok(merged)
        }
        Err(e) => {
            warn!("Failed to sync settings from cloud: {}", e);
            // Return local settings if sync fails
            Ok(local.clone())
        }
    }
}

/// Push local settings to cloud
pub async fn push_settings_to_cloud(api: &SyncApi, settings: &Settings) -> Result<()> {
    info!("Pushing settings to cloud...");
    
    let updates = settings_to_update(settings);
    
    match api.update_settings(updates).await {
        Ok(_) => {
            info!("Settings pushed to cloud successfully");
            Ok(())
        }
        Err(e) => {
            error!("Failed to push settings to cloud: {}", e);
            Err(e)
        }
    }
}