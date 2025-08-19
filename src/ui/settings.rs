use anyhow::Result;
use eframe::egui;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, error};
use tokio::sync::mpsc;
use crate::storage::SettingsManager;

#[derive(Debug, Clone)]
pub struct Settings {
    pub auto_save_enabled: bool,
    pub save_interval_minutes: u32,
    pub max_saves_per_game: u32,
    pub start_on_boot: bool,
    pub minimize_to_tray: bool,
    pub show_notifications: bool,
    pub cloud_sync_enabled: bool,
    pub cloud_api_url: String,
    pub cloud_auto_sync: bool,
    pub hotkey_enabled: bool,
    pub save_hotkey: Option<String>,
    pub compression_enabled: bool,
    pub compression_level: i32,
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
            cloud_api_url: "http://localhost:3000".to_string(),
            cloud_auto_sync: true,
            hotkey_enabled: true,
            save_hotkey: Some("Ctrl+Shift+S".to_string()),
            compression_enabled: true,
            compression_level: 3,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SettingsCommand {
    Show,
    Hide,
}

pub struct SettingsWindow {
    command_sender: mpsc::Sender<SettingsCommand>,
    settings: Arc<Mutex<Settings>>,
    settings_manager: Option<Arc<SettingsManager>>,
}

impl SettingsWindow {
    pub fn new() -> Result<Self> {
        Self::new_with_settings(Settings::default())
    }
    
    pub fn new_with_settings(initial_settings: Settings) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<SettingsCommand>(10);
        let settings = Arc::new(Mutex::new(initial_settings));
        let settings_clone = settings.clone();
        
        // Start the settings window in a dedicated thread
        std::thread::spawn(move || {
            if let Err(e) = Self::run_window(settings_clone, rx, None) {
                error!("Settings window thread error: {}", e);
            }
        });
        
        Ok(Self {
            command_sender: tx,
            settings,
            settings_manager: None,
        })
    }
    
    pub fn with_settings_manager(initial_settings: Settings, settings_manager: Arc<SettingsManager>) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<SettingsCommand>(10);
        let settings = Arc::new(Mutex::new(initial_settings));
        let settings_clone = settings.clone();
        let settings_manager_clone = Some(settings_manager.clone());
        
        // Start the settings window in a dedicated thread
        std::thread::spawn(move || {
            if let Err(e) = Self::run_window(settings_clone, rx, settings_manager_clone) {
                error!("Settings window thread error: {}", e);
            }
        });
        
        Ok(Self {
            command_sender: tx,
            settings,
            settings_manager: Some(settings_manager),
        })
    }
    
    fn run_window(
        settings: Arc<Mutex<Settings>>,
        command_receiver: mpsc::Receiver<SettingsCommand>,
        settings_manager: Option<Arc<SettingsManager>>,
    ) -> Result<()> {
        // Wait for the first Show command before creating the window
        let runtime = tokio::runtime::Runtime::new()?;
        
        runtime.block_on(async move {
            let mut command_receiver = command_receiver;
            // Wait for first show command
            info!("Settings window thread waiting for first show command");
            loop {
                if let Some(cmd) = command_receiver.recv().await {
                    match cmd {
                        SettingsCommand::Show => {
                            info!("First show command received, creating settings window");
                            break;
                        }
                        SettingsCommand::Hide => {
                            // Ignore hide commands before window exists
                            continue;
                        }
                    }
                } else {
                    // Channel closed, exit
                    return Ok(());
                }
            }
            
            // Now create the channel for the window
            let (tx, rx) = std::sync::mpsc::channel::<SettingsCommand>();
            
            // Forward remaining commands to the window
            let tx_clone = tx.clone();
            tokio::spawn(async move {
                while let Some(cmd) = command_receiver.recv().await {
                    if tx_clone.send(cmd).is_err() {
                        break;
                    }
                }
            });
            
            // Load window icon
            let mut viewport = egui::ViewportBuilder::default()
                .with_inner_size([500.0, 400.0])
                .with_resizable(false)
                .with_visible(true); // Show immediately since we got a Show command
                
            if let Some(icon) = Self::load_window_icon() {
                viewport = viewport.with_icon(std::sync::Arc::new(icon));
            }
            
            // Configure native options with Linux-specific settings
            let native_options = eframe::NativeOptions {
                viewport,
                #[cfg(target_os = "linux")]
                event_loop_builder: Some(Box::new(|builder| {
                    use winit::platform::x11::EventLoopBuilderExtX11;
                    builder.with_any_thread(true);
                })),
                ..Default::default()
            };

            // Run the event loop
            eframe::run_native(
                "Retrosave Settings",
                native_options,
                Box::new(move |_cc| {
                    Ok(Box::new(SettingsApp {
                        settings: settings.clone(),
                        command_receiver: rx,
                        visible: true, // Start visible since we're responding to Show
                        first_show: false, // Already showing
                        settings_manager: settings_manager.clone(),
                    }))
                }),
            ).map_err(|e| anyhow::anyhow!("Failed to run settings window: {}", e))?;

            Ok(())
        })
    }
    
    pub async fn show(&self) -> Result<()> {
        info!("Showing settings window");
        self.command_sender.send(SettingsCommand::Show).await
            .map_err(|_| anyhow::anyhow!("Failed to send show command"))?;
        Ok(())
    }
    
    pub async fn hide(&self) -> Result<()> {
        info!("Hiding settings window");
        self.command_sender.send(SettingsCommand::Hide).await
            .map_err(|_| anyhow::anyhow!("Failed to send hide command"))?;
        Ok(())
    }
    
    pub fn get_settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
    
    fn load_window_icon() -> Option<egui::IconData> {
        // Try to load icon from various possible paths
        let icon_paths = [
            "/home/eralp/Projects/retrosave/client/assets/icon-256.png",
            "/home/eralp/Projects/retrosave/client/assets/icon-128.png",
            "/home/eralp/Projects/retrosave/client/assets/icon-64.png",
            "client/assets/icon-256.png",
            "client/assets/icon-128.png",
            "client/assets/icon-64.png",
            "assets/icon-256.png",
            "assets/icon-128.png",
            "assets/icon-64.png",
        ];
        
        for path in &icon_paths {
            if let Ok(image_data) = std::fs::read(path) {
                if let Ok(image) = image::load_from_memory(&image_data) {
                    let rgba = image.to_rgba8();
                    let (width, height) = rgba.dimensions();
                    return Some(egui::IconData {
                        rgba: rgba.into_raw(),
                        width,
                        height,
                    });
                }
            }
        }
        
        // If no icon file found, create a simple colored icon as fallback
        info!("Using fallback icon for settings window");
        let size = 64;
        let mut rgba = Vec::with_capacity((size * size * 4) as usize);
        for _ in 0..size*size {
            rgba.push(34);   // R - dark green
            rgba.push(139);  // G
            rgba.push(34);   // B
            rgba.push(255);  // A - fully opaque
        }
        
        Some(egui::IconData {
            rgba,
            width: size,
            height: size,
        })
    }
}

struct SettingsApp {
    settings: Arc<Mutex<Settings>>,
    command_receiver: std::sync::mpsc::Receiver<SettingsCommand>,
    visible: bool,
    first_show: bool,
    settings_manager: Option<Arc<SettingsManager>>,
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for commands
        if let Ok(cmd) = self.command_receiver.try_recv() {
            match cmd {
                SettingsCommand::Show => {
                    info!("Settings window received show command");
                    self.visible = true;
                    if self.first_show {
                        // On first show, make window visible
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        self.first_show = false;
                    } else {
                        // On subsequent shows, just unhide and focus
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
                SettingsCommand::Hide => {
                    info!("Settings window received hide command");
                    self.visible = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
        }
        
        // Request repaint to keep checking for commands
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        
        // Only show UI if visible
        if !self.visible {
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
            
            // Hotkey Settings
            ui.label("Hotkey Settings");
            ui.checkbox(&mut settings.hotkey_enabled, "Enable global hotkeys");
            
            if settings.hotkey_enabled {
                ui.horizontal(|ui| {
                    ui.label("Save Now hotkey:");
                    
                    let mut hotkey_text = settings.save_hotkey.clone().unwrap_or_else(|| "Not set".to_string());
                    let response = ui.text_edit_singleline(&mut hotkey_text);
                    
                    if response.changed() {
                        // Update the hotkey when text changes
                        if hotkey_text.is_empty() || hotkey_text == "Not set" {
                            settings.save_hotkey = None;
                        } else {
                            settings.save_hotkey = Some(hotkey_text);
                        }
                    }
                    
                    if ui.button("Clear").clicked() {
                        settings.save_hotkey = None;
                    }
                });
                
                ui.label("Format: Ctrl+Shift+S, Alt+F5, etc.");
                ui.label("The hotkey will trigger a manual save while in-game.");
            }
            
            ui.separator();
            
            // Cloud Settings
            ui.heading("Cloud Sync");
            ui.checkbox(&mut settings.cloud_sync_enabled, "Enable cloud sync");
            
            if settings.cloud_sync_enabled {
                ui.indent("cloud_settings", |ui| {
                    // API URL
                    ui.horizontal(|ui| {
                        ui.label("API URL:");
                        ui.text_edit_singleline(&mut settings.cloud_api_url);
                    });
                    
                    // Auto sync
                    ui.checkbox(&mut settings.cloud_auto_sync, "Automatically sync saves");
                    
                    // Auth status (placeholder for now)
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Status:");
                        ui.colored_label(egui::Color32::from_rgb(255, 100, 100), "Not logged in");
                    });
                    
                    // Login/Register buttons
                    ui.horizontal(|ui| {
                        if ui.button("Login").clicked() {
                            // TODO: Open login dialog
                            info!("Login button clicked");
                        }
                        if ui.button("Register").clicked() {
                            // TODO: Open register dialog
                            info!("Register button clicked");
                        }
                    });
                    
                    ui.label("💡 Cloud sync keeps your saves synchronized across all devices");
                });
            }
            
            ui.separator();
            
            ui.heading("Compression");
            ui.checkbox(&mut settings.compression_enabled, "Enable save compression");
            if settings.compression_enabled {
                ui.horizontal(|ui| {
                    ui.label("Compression level:");
                    ui.add(egui::Slider::new(&mut settings.compression_level, 1..=22)
                        .text("Level"));
                });
                ui.label(format!("Level {}: {}", 
                    settings.compression_level,
                    match settings.compression_level {
                        1..=3 => "Fast (less compression)",
                        4..=9 => "Balanced",
                        10..=15 => "Good compression",
                        16..=22 => "Best compression (slower)",
                        _ => "Unknown"
                    }
                ));
                ui.label("💡 Level 3 recommended for best speed/size balance");
            }
            
            ui.separator();
            
            // Buttons (Only Save and Cancel - no confusing Apply button)
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    info!("Settings saved button clicked");
                    // Clone settings while we have the lock, but drop it before spawning thread
                    let settings_to_save = settings.clone();
                    drop(settings); // Explicitly drop the mutex guard
                    
                    // Save directly using settings manager if available
                    if let Some(ref manager) = self.settings_manager {
                        let manager_clone = manager.clone();
                        
                        // Spawn a thread to do the async save without blocking UI
                        std::thread::spawn(move || {
                            // Create a small runtime just for this save operation
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                info!("Saving settings to database...");
                                if let Err(e) = manager_clone.save_settings(&settings_to_save).await {
                                    error!("Failed to save settings: {}", e);
                                } else {
                                    info!("Settings successfully saved to database");
                                }
                            });
                        });
                    }
                    
                    self.visible = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                
                if ui.button("Cancel").clicked() {
                    debug!("Settings cancelled");
                    self.visible = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            });
        });
        
        // Handle window close button
        if ctx.input(|i| i.viewport().close_requested()) {
            self.visible = false;
            // Don't actually close, just hide
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }
}