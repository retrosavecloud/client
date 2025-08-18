use notify_rust::{Notification, Timeout};
use tracing::{debug, warn};

pub struct NotificationManager {
    enabled: bool,
    app_name: String,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            enabled: true,
            app_name: "Retrosave".to_string(),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        debug!("Notifications {}", if enabled { "enabled" } else { "disabled" });
    }

    pub fn show_info(&self, title: &str, message: &str) {
        if !self.enabled {
            return;
        }

        if let Err(e) = Notification::new()
            .summary(title)
            .body(message)
            .appname(&self.app_name)
            .icon("dialog-information")
            .timeout(Timeout::Milliseconds(5000))
            .show()
        {
            warn!("Failed to show notification: {}", e);
        } else {
            debug!("Showed notification: {} - {}", title, message);
        }
    }

    pub fn show_success(&self, title: &str, message: &str) {
        if !self.enabled {
            return;
        }

        if let Err(e) = Notification::new()
            .summary(title)
            .body(message)
            .appname(&self.app_name)
            .icon("dialog-positive")
            .timeout(Timeout::Milliseconds(5000))
            .show()
        {
            warn!("Failed to show notification: {}", e);
        } else {
            debug!("Showed success notification: {} - {}", title, message);
        }
    }

    pub fn show_warning(&self, title: &str, message: &str) {
        if !self.enabled {
            return;
        }

        if let Err(e) = Notification::new()
            .summary(title)
            .body(message)
            .appname(&self.app_name)
            .icon("dialog-warning")
            .timeout(Timeout::Milliseconds(7000))
            .show()
        {
            warn!("Failed to show notification: {}", e);
        } else {
            debug!("Showed warning notification: {} - {}", title, message);
        }
    }

    pub fn show_error(&self, title: &str, message: &str) {
        if !self.enabled {
            return;
        }

        if let Err(e) = Notification::new()
            .summary(title)
            .body(message)
            .appname(&self.app_name)
            .icon("dialog-error")
            .timeout(Timeout::Milliseconds(10000))
            .show()
        {
            warn!("Failed to show notification: {}", e);
        } else {
            debug!("Showed error notification: {} - {}", title, message);
        }
    }

    // Specific notifications for Retrosave events
    pub fn notify_emulator_detected(&self, emulator: &str) {
        self.show_info(
            "Emulator Detected",
            &format!("{} is now running. Save monitoring active.", emulator),
        );
    }

    pub fn notify_emulator_stopped(&self, emulator: &str) {
        self.show_info(
            "Emulator Stopped",
            &format!("{} has stopped. Save monitoring paused.", emulator),
        );
    }

    pub fn notify_game_detected(&self, game: &str) {
        self.show_info(
            "Game Detected",
            &format!("Now playing: {}", game),
        );
    }

    pub fn notify_save_detected(&self, game: &str) {
        self.show_success(
            "Game Saved",
            &format!("{} progress saved and backed up", game),
        );
    }

    pub fn notify_manual_save(&self) {
        self.show_info(
            "Manual Save",
            "Checking for save file changes...",
        );
    }

    pub fn notify_settings_saved(&self) {
        self.show_success(
            "Settings",
            "Settings have been saved successfully",
        );
    }
}