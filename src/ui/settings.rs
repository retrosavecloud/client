use anyhow::Result;
use eframe::egui;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, error};
use tokio::sync::mpsc;
use crate::storage::SettingsManager;
use crate::sync::AuthManager;

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
            if let Err(e) = Self::run_window(settings_clone, rx, None, None) {
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
            if let Err(e) = Self::run_window(settings_clone, rx, settings_manager_clone, None) {
                error!("Settings window thread error: {}", e);
            }
        });
        
        Ok(Self {
            command_sender: tx,
            settings,
            settings_manager: Some(settings_manager),
        })
    }
    
    pub fn with_auth_manager(
        initial_settings: Settings, 
        settings_manager: Arc<SettingsManager>,
        auth_manager: Arc<AuthManager>,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<SettingsCommand>(10);
        let settings = Arc::new(Mutex::new(initial_settings));
        let settings_clone = settings.clone();
        let settings_manager_clone = Some(settings_manager.clone());
        let auth_manager_clone = Some(auth_manager.clone());
        
        // Start the settings window in a dedicated thread
        std::thread::spawn(move || {
            if let Err(e) = Self::run_window(settings_clone, rx, settings_manager_clone, auth_manager_clone) {
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
        auth_manager: Option<Arc<AuthManager>>,
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
                .with_inner_size([650.0, 800.0])  // Increased width and height
                .with_resizable(true)
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
                Box::new(move |cc| {
                    // Load custom font
                    Self::setup_custom_fonts(&cc.egui_ctx);
                    // Check auth state right when creating the app
                    let (is_authenticated, user_email, auth_check_rx) = if let Some(ref auth_manager) = auth_manager {
                        let auth_manager_clone = auth_manager.clone();
                        let (tx, rx) = std::sync::mpsc::channel();
                        
                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                let is_auth = auth_manager_clone.is_authenticated().await;
                                if is_auth {
                                    let user = auth_manager_clone.get_user_info().await;
                                    let _ = tx.send((true, user.map(|u| u.email)));
                                } else {
                                    let _ = tx.send((false, None));
                                }
                            });
                        });
                        
                        // Try to get immediate result, otherwise store receiver
                        match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                            Ok((is_auth, email)) => {
                                (is_auth, email, None)
                            },
                            Err(_) => {
                                (false, None, Some(rx))
                            }
                        }
                    } else {
                        (false, None, None)
                    };
                    
                    Ok(Box::new(SettingsApp {
                        settings: settings.clone(),
                        command_receiver: rx,
                        visible: true, // Start visible since we're responding to Show
                        first_show: false, // Already showing
                        settings_manager: settings_manager.clone(),
                        auth_manager: auth_manager.clone(),
                        // Auth state
                        is_authenticated,
                        user_email,
                        // Auth form state
                        auth_mode: AuthMode::Login,
                        auth_email: String::new(),
                        auth_username: String::new(),
                        auth_password: String::new(),
                        auth_confirm_password: String::new(),
                        auth_is_loading: false,
                        auth_error: None,
                        auth_result_rx: None,
                        auth_check_rx,
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
    
    fn setup_custom_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        
        // Load Inter font
        let font_path = std::path::Path::new("client/assets/fonts/Inter-Regular.ttf");
        if let Ok(font_data) = std::fs::read(font_path) {
            fonts.font_data.insert(
                "Inter".to_owned(),
                egui::FontData::from_owned(font_data),
            );
            
            // Insert Inter as the first priority for all font families
            fonts.families.entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "Inter".to_owned());
            
            fonts.families.entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "Inter".to_owned());
                
            ctx.set_fonts(fonts);
            
            // Also set a slightly larger default text size
            let mut style = (*ctx.style()).clone();
            style.text_styles = [
                (egui::TextStyle::Small, egui::FontId::new(11.0, egui::FontFamily::Proportional)),
                (egui::TextStyle::Body, egui::FontId::new(13.0, egui::FontFamily::Proportional)),
                (egui::TextStyle::Button, egui::FontId::new(13.0, egui::FontFamily::Proportional)),
                (egui::TextStyle::Heading, egui::FontId::new(18.0, egui::FontFamily::Proportional)),
                (egui::TextStyle::Monospace, egui::FontId::new(12.0, egui::FontFamily::Monospace)),
            ].iter().cloned().collect();
            ctx.set_style(style);
        }
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
    auth_manager: Option<Arc<AuthManager>>,
    // Auth state
    is_authenticated: bool,
    user_email: Option<String>,
    // Auth form state
    auth_mode: AuthMode,
    auth_email: String,
    auth_username: String,
    auth_password: String,
    auth_confirm_password: String,
    auth_is_loading: bool,
    auth_error: Option<String>,
    auth_result_rx: Option<std::sync::mpsc::Receiver<AuthResult>>,
    auth_check_rx: Option<std::sync::mpsc::Receiver<(bool, Option<String>)>>,
}

#[derive(Debug, Clone, PartialEq)]
enum AuthMode {
    Login,
    Register,
}

#[derive(Debug, Clone)]
enum AuthResult {
    Success { email: String },
    Error(String),
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
                    
                    // Check auth status when showing window
                    if let Some(ref auth_manager) = self.auth_manager {
                        let auth_manager_clone = auth_manager.clone();
                        let (tx, rx) = std::sync::mpsc::channel();
                        
                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                let is_auth = auth_manager_clone.is_authenticated().await;
                                if is_auth {
                                    let user = auth_manager_clone.get_user_info().await;
                                    let _ = tx.send((true, user.map(|u| u.email)));
                                } else {
                                    let _ = tx.send((false, None));
                                }
                            });
                        });
                        
                        // Store the receiver to check later
                        self.auth_check_rx = Some(rx);
                        ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    }
                }
                SettingsCommand::Hide => {
                    info!("Settings window received hide command");
                    self.visible = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
        }
        
        // Check for auth status check results
        if let Some(ref rx) = self.auth_check_rx {
            if let Ok((is_auth, email)) = rx.try_recv() {
                self.is_authenticated = is_auth;
                self.user_email = email;
                self.auth_check_rx = None; // Clear after receiving
                ctx.request_repaint();
            }
        }
        
        // Check for auth results
        if let Some(ref rx) = self.auth_result_rx {
            if let Ok(result) = rx.try_recv() {
                self.auth_is_loading = false;
                match result {
                    AuthResult::Success { email } => {
                        info!("Authentication successful for {}", email);
                        self.is_authenticated = true;
                        self.user_email = Some(email);
                        self.auth_error = None;
                        // Clear form
                        self.auth_password.clear();
                        self.auth_confirm_password.clear();
                    }
                    AuthResult::Error(err) => {
                        self.auth_error = Some(err);
                    }
                }
                self.auth_result_rx = None;
                ctx.request_repaint();
            }
        }
        
        // Periodically check auth state from auth manager
        static mut LAST_AUTH_CHECK: Option<std::time::Instant> = None;
        let should_check = unsafe {
            match LAST_AUTH_CHECK {
                None => {
                    LAST_AUTH_CHECK = Some(std::time::Instant::now());
                    true
                }
                Some(last) => {
                    if last.elapsed() > std::time::Duration::from_secs(5) {
                        LAST_AUTH_CHECK = Some(std::time::Instant::now());
                        true
                    } else {
                        false
                    }
                }
            }
        };
        
        if should_check && self.visible {
            if let Some(ref auth_manager) = self.auth_manager {
                let auth_manager_clone = auth_manager.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        let is_auth = auth_manager_clone.is_authenticated().await;
                        if is_auth {
                            if let Some(user) = auth_manager_clone.get_user_info().await {
                                debug!("Periodic auth check: authenticated as {}", user.email);
                            }
                        }
                    });
                });
            }
        }
        
        // Request repaint to keep checking for commands
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        
        // Only show UI if visible
        if !self.visible {
            return;
        }
        
        // Action flags to avoid borrow checker issues
        let mut should_logout = false;
        let mut should_perform_login = false;
        let mut should_perform_register = false;
        
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Retrosave Settings");
            ui.separator();
            
            // Wrap everything in a scroll area
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_min_width(550.0);
                
                // Settings sections with proper locking
                let mut cloud_sync_enabled;
                {
                    let mut settings = self.settings.lock().unwrap();
            
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
            
            cloud_sync_enabled = settings.cloud_sync_enabled;
            } // Drop settings lock
            
            // Always show auth status
            if self.is_authenticated {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(40, 45, 40))
                    .rounding(egui::Rounding::same(5.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            // First row: Status
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("✅").size(20.0));
                                ui.label(egui::RichText::new("Connected").color(egui::Color32::from_rgb(100, 255, 100)).size(14.0));
                                if let Some(ref email) = self.user_email {
                                    ui.label(egui::RichText::new(format!(" - {}", email)).size(12.0));
                                }
                            });
                            
                            // Second row: Buttons
                            ui.add_space(5.0);
                            ui.horizontal(|ui| {
                                if ui.button("🔄 Sync Now").clicked() {
                                    info!("Manual sync requested");
                                }
                                if ui.button("📤 Logout").clicked() {
                                    should_logout = true;
                                }
                            });
                        });
                    });
                ui.add_space(10.0);
                
                // Cloud sync checkbox for authenticated users
                {
                    let mut settings = self.settings.lock().unwrap();
                    ui.checkbox(&mut settings.cloud_sync_enabled, "Enable cloud sync");
                    cloud_sync_enabled = settings.cloud_sync_enabled;
                }
            } else {
                // Not authenticated - show login form directly
                ui.label(egui::RichText::new("🔒 Sign in to enable cloud sync").color(egui::Color32::from_rgb(200, 200, 200)));
                ui.add_space(10.0);
                
                // Show auth forms
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(45, 45, 50))
                            .rounding(egui::Rounding::same(5.0))
                            .inner_margin(egui::Margin::same(12.0))
                            .show(ui, |ui| {
                                // Mode selector
                                ui.horizontal(|ui| {
                                    ui.label("Account:");
                                    if ui.selectable_label(self.auth_mode == AuthMode::Login, "Sign In").clicked() {
                                        self.auth_mode = AuthMode::Login;
                                        self.auth_error = None;
                                    }
                                    ui.label("|");
                                    if ui.selectable_label(self.auth_mode == AuthMode::Register, "Sign Up").clicked() {
                                        self.auth_mode = AuthMode::Register;
                                        self.auth_error = None;
                                    }
                                });
                                
                                ui.add_space(8.0);
                                
                                // Error message
                                if let Some(ref error) = self.auth_error {
                                    ui.colored_label(egui::Color32::from_rgb(255, 100, 100), format!("⚠ {}", error));
                                    ui.add_space(5.0);
                                }
                                
                                // Show appropriate form
                                match self.auth_mode {
                                    AuthMode::Login => {
                                        if self.show_login_form_ui(ui) {
                                            should_perform_login = true;
                                        }
                                    },
                                    AuthMode::Register => {
                                        if self.show_register_form_ui(ui) {
                                            should_perform_register = true;
                                        }
                                    },
                                }
                            });
            }
            
            // Cloud sync settings if authenticated and enabled
            if self.is_authenticated && cloud_sync_enabled {
                ui.indent("cloud_settings", |ui| {
                    // API URL and auto sync
                    {
                        let mut settings = self.settings.lock().unwrap();
                        ui.horizontal(|ui| {
                            ui.label("API URL:");
                            ui.text_edit_singleline(&mut settings.cloud_api_url);
                        });
                        
                        // Auto sync
                        ui.checkbox(&mut settings.cloud_auto_sync, "Automatically sync saves");
                    }
                    
                    ui.label("💡 Cloud sync keeps your saves synchronized across all devices");
                });
            }
            
            ui.separator();
            
            ui.heading("Compression");
            {
                let mut settings = self.settings.lock().unwrap();
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
            } // Drop settings lock
            
            // Add some space before buttons
            ui.add_space(20.0);
            
            // Buttons at the bottom inside scroll area
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    info!("Settings saved button clicked");
                    // Clone settings for saving
                    let settings_to_save = self.settings.lock().unwrap().clone();
                    
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
            
            }); // End of ScrollArea
        });
        
        // Handle window close button
        if ctx.input(|i| i.viewport().close_requested()) {
            self.visible = false;
            // Don't actually close, just hide
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        
        // Handle deferred actions
        if should_logout {
            self.perform_logout(ctx);
        }
        if should_perform_login {
            self.perform_login(ctx);
        }
        if should_perform_register {
            self.perform_register(ctx);
        }
    }
}

impl SettingsApp {
    fn show_login_form_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut should_login = false;
        
        // Email field
        ui.label("Email:");
        let email_response = ui.text_edit_singleline(&mut self.auth_email);
        
        ui.add_space(5.0);
        
        // Password field
        ui.label("Password:");
        let password_response = ui.add(
            egui::TextEdit::singleline(&mut self.auth_password)
                .password(true)
        );
        
        ui.add_space(10.0);
        
        // Submit on Enter
        if (email_response.lost_focus() || password_response.lost_focus()) 
            && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            should_login = true;
        }
        
        // Login button
        ui.add_enabled_ui(!self.auth_is_loading, |ui| {
            let button_text = if self.auth_is_loading { "Signing in..." } else { "🔐 Sign In" };
            if ui.button(egui::RichText::new(button_text).size(14.0)).clicked() {
                should_login = true;
            }
        });
        
        should_login
    }
    
    fn show_register_form_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut should_register = false;
        
        // Username field
        ui.label("Username:");
        ui.text_edit_singleline(&mut self.auth_username);
        
        ui.add_space(5.0);
        
        // Email field
        ui.label("Email:");
        ui.text_edit_singleline(&mut self.auth_email);
        
        ui.add_space(5.0);
        
        // Password field
        ui.label("Password:");
        ui.add(
            egui::TextEdit::singleline(&mut self.auth_password)
                .password(true)
        );
        
        ui.add_space(5.0);
        
        // Confirm password field
        ui.label("Confirm Password:");
        let confirm_response = ui.add(
            egui::TextEdit::singleline(&mut self.auth_confirm_password)
                .password(true)
        );
        
        ui.add_space(10.0);
        
        // Submit on Enter
        if confirm_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            should_register = true;
        }
        
        // Register button
        ui.add_enabled_ui(!self.auth_is_loading, |ui| {
            let button_text = if self.auth_is_loading { "Creating..." } else { "✨ Create Account" };
            if ui.button(egui::RichText::new(button_text).size(14.0)).clicked() {
                should_register = true;
            }
        });
        
        should_register
    }
    
    fn perform_login(&mut self, ctx: &egui::Context) {
        // Validate
        if self.auth_email.is_empty() || self.auth_password.is_empty() {
            self.auth_error = Some("Please fill in all fields".to_string());
            return;
        }
        
        self.auth_is_loading = true;
        self.auth_error = None;
        
        // Create channel for result
        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_result_rx = Some(rx);
        
        if let Some(ref auth_manager) = self.auth_manager {
            let auth_manager = auth_manager.clone();
            let email = self.auth_email.clone();
            let password = self.auth_password.clone();
            
            // Perform login in background
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    match auth_manager.login(&email, &password).await {
                        Ok(_) => {
                            let _ = tx.send(AuthResult::Success { email });
                        }
                        Err(e) => {
                            let error_msg = if e.to_string().contains("401") {
                                "Invalid email or password".to_string()
                            } else if e.to_string().contains("Failed to send") {
                                "Cannot connect to server".to_string()
                            } else {
                                format!("Login failed: {}", e)
                            };
                            let _ = tx.send(AuthResult::Error(error_msg));
                        }
                    }
                });
            });
        }
        
        ctx.request_repaint();
    }
    
    fn perform_register(&mut self, ctx: &egui::Context) {
        // Validate
        if self.auth_username.is_empty() || self.auth_email.is_empty() || 
           self.auth_password.is_empty() || self.auth_confirm_password.is_empty() {
            self.auth_error = Some("Please fill in all fields".to_string());
            return;
        }
        
        if self.auth_password != self.auth_confirm_password {
            self.auth_error = Some("Passwords do not match".to_string());
            return;
        }
        
        if self.auth_password.len() < 8 {
            self.auth_error = Some("Password must be at least 8 characters".to_string());
            return;
        }
        
        self.auth_is_loading = true;
        self.auth_error = None;
        
        // Create channel for result
        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_result_rx = Some(rx);
        
        if let Some(ref auth_manager) = self.auth_manager {
            let auth_manager = auth_manager.clone();
            let username = self.auth_username.clone();
            let email = self.auth_email.clone();
            let password = self.auth_password.clone();
            
            // Perform registration in background
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    match auth_manager.register(&email, &username, &password).await {
                        Ok(_) => {
                            let _ = tx.send(AuthResult::Success { email });
                        }
                        Err(e) => {
                            let error_msg = if e.to_string().contains("already exists") {
                                "Email or username already exists".to_string()
                            } else if e.to_string().contains("Failed to send") {
                                "Cannot connect to server".to_string()
                            } else {
                                format!("Registration failed: {}", e)
                            };
                            let _ = tx.send(AuthResult::Error(error_msg));
                        }
                    }
                });
            });
        }
        
        ctx.request_repaint();
    }
    
    fn perform_logout(&mut self, ctx: &egui::Context) {
        if let Some(ref auth_manager) = self.auth_manager {
            let auth_manager_clone = auth_manager.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    if let Err(e) = auth_manager_clone.logout().await {
                        error!("Logout failed: {}", e);
                    } else {
                        info!("Logged out successfully");
                    }
                });
            });
            self.is_authenticated = false;
            self.user_email = None;
        }
        ctx.request_repaint();
    }
}