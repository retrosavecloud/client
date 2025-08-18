use anyhow::Result;
use eframe::egui;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct Settings {
    pub auto_save_enabled: bool,
    pub save_interval_minutes: u32,
    pub max_saves_per_game: u32,
    pub start_on_boot: bool,
    pub minimize_to_tray: bool,
    pub show_notifications: bool,
    pub cloud_sync_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_save_enabled: true,
            save_interval_minutes: 5,
            max_saves_per_game: 5,
            start_on_boot: false,
            minimize_to_tray: true,
            show_notifications: true,
            cloud_sync_enabled: false,
        }
    }
}

pub struct SettingsWindow {
    settings: Arc<Mutex<Settings>>,
    visible: Arc<Mutex<bool>>,
}

impl SettingsWindow {
    pub fn new() -> Self {
        Self {
            settings: Arc::new(Mutex::new(Settings::default())),
            visible: Arc::new(Mutex::new(false)),
        }
    }

    pub fn show(&self) {
        *self.visible.lock().unwrap() = true;
        info!("Settings window opened");
    }

    pub fn hide(&self) {
        *self.visible.lock().unwrap() = false;
        info!("Settings window closed");
    }

    pub fn is_visible(&self) -> bool {
        *self.visible.lock().unwrap()
    }

    pub fn run(&self) -> Result<()> {
        let settings = self.settings.clone();
        let visible = self.visible.clone();
        
        let native_options = eframe::NativeOptions {
            initial_window_size: Some(egui::Vec2::new(500.0, 400.0)),
            resizable: false,
            always_on_top: false,
            ..Default::default()
        };

        eframe::run_native(
            "Retrosave Settings",
            native_options,
            Box::new(move |cc| {
                Ok(Box::new(SettingsApp {
                    settings: settings.clone(),
                    visible: visible.clone(),
                }))
            }),
        )?;

        Ok(())
    }
}

struct SettingsApp {
    settings: Arc<Mutex<Settings>>,
    visible: Arc<Mutex<bool>>,
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Check if window should be visible
        if !*self.visible.lock().unwrap() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut settings = self.settings.lock().unwrap();
            
            ui.heading("Retrosave Settings");
            ui.separator();
            
            // General Settings
            ui.label("General Settings");
            ui.checkbox(&mut settings.auto_save_enabled, "Enable automatic saves");
            
            ui.horizontal(|ui| {
                ui.label("Save interval (minutes):");
                ui.add(egui::Slider::new(&mut settings.save_interval_minutes, 1..=60));
            });
            
            ui.horizontal(|ui| {
                ui.label("Max saves per game:");
                ui.add(egui::Slider::new(&mut settings.max_saves_per_game, 1..=20));
            });
            
            ui.separator();
            
            // System Settings
            ui.label("System Settings");
            ui.checkbox(&mut settings.start_on_boot, "Start Retrosave on system boot");
            ui.checkbox(&mut settings.minimize_to_tray, "Minimize to system tray");
            ui.checkbox(&mut settings.show_notifications, "Show notifications");
            
            ui.separator();
            
            // Cloud Settings
            ui.label("Cloud Settings");
            ui.checkbox(&mut settings.cloud_sync_enabled, "Enable cloud sync (coming soon)");
            if settings.cloud_sync_enabled {
                ui.label("Cloud sync will be available in v1.1");
                settings.cloud_sync_enabled = false;
            }
            
            ui.separator();
            
            // Buttons
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    info!("Settings saved");
                    // TODO: Actually save settings to database
                    *self.visible.lock().unwrap() = false;
                }
                
                if ui.button("Cancel").clicked() {
                    debug!("Settings cancelled");
                    *self.visible.lock().unwrap() = false;
                }
                
                if ui.button("Apply").clicked() {
                    info!("Settings applied");
                    // TODO: Apply settings without closing
                }
            });
        });
    }
}