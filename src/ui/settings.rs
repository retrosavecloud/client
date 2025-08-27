use anyhow::Result;
use eframe::egui;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, error};
use tokio::sync::mpsc;
use crate::storage::SettingsManager;
use crate::sync::{AuthManager, api::SyncApi, WebSocketClient};
use crate::payment::{SubscriptionStatus, UsageStats};

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
        // Get API URL from environment or use default based on build mode
        let cloud_api_url = std::env::var("RETROSAVE_API_URL")
            .unwrap_or_else(|_| {
                // In debug mode, use localhost
                #[cfg(debug_assertions)]
                return "http://localhost:8080".to_string();
                
                // In release mode, use production API
                #[cfg(not(debug_assertions))]
                return "https://api.retrosave.cloud".to_string();
            });

        Self {
            auto_save_enabled: true,
            save_interval_minutes: 5,
            max_saves_per_game: 5,
            start_on_boot: false,
            minimize_to_tray: true,
            show_notifications: true,
            cloud_sync_enabled: false,
            cloud_api_url,
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
            if let Err(e) = Self::run_window(settings_clone, rx, None, None, None) {
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
            if let Err(e) = Self::run_window(settings_clone, rx, settings_manager_clone, None, None) {
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
        let settings = Arc::new(Mutex::new(initial_settings.clone()));
        let settings_clone = settings.clone();
        let settings_manager_clone = Some(settings_manager.clone());
        let auth_manager_clone = Some(auth_manager.clone());
        
        // Create API client
        let api_client = Some(Arc::new(SyncApi::new(
            initial_settings.cloud_api_url.clone(),
            auth_manager.clone(),
        )));
        
        // Start the settings window in a dedicated thread
        std::thread::spawn(move || {
            if let Err(e) = Self::run_window(settings_clone, rx, settings_manager_clone, auth_manager_clone, api_client) {
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
        api_client: Option<Arc<SyncApi>>,
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
                    
                    let mut app = SettingsApp {
                        settings: settings.clone(),
                        command_receiver: rx,
                        visible: true, // Start visible since we're responding to Show
                        first_show: false, // Already showing
                        settings_manager: settings_manager.clone(),
                        auth_manager: auth_manager.clone(),
                        // Auth state
                        is_authenticated,
                        user_email: user_email.clone(),
                        // Auth state
                        auth_is_loading: false,
                        auth_error: None,
                        auth_result_rx: None,
                        auth_check_rx,
                        // Subscription state
                        api_client: api_client.clone(),
                        subscription_status: None,
                        usage_stats: None,
                        subscription_loading: false,
                        subscription_rx: None,
                        // WebSocket client
                        ws_client: None,
                        ws_initialized: false,
                        ws_subscription_rx: None,
                        ws_usage_rx: None,
                    };
                    
                    // If authenticated on startup, fetch subscription status
                    if is_authenticated && api_client.is_some() {
                        info!("User authenticated on window creation ({}), fetching subscription", user_email.as_ref().unwrap_or(&"unknown".to_string()));
                        app.fetch_subscription_status(&cc.egui_ctx);
                    }
                    
                    Ok(Box::new(app))
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
    auth_is_loading: bool,
    auth_error: Option<String>,
    auth_result_rx: Option<std::sync::mpsc::Receiver<AuthResult>>,
    auth_check_rx: Option<std::sync::mpsc::Receiver<(bool, Option<String>)>>,
    // Subscription state
    api_client: Option<Arc<SyncApi>>,
    subscription_status: Option<SubscriptionStatus>,
    usage_stats: Option<UsageStats>,
    subscription_loading: bool,
    subscription_rx: Option<std::sync::mpsc::Receiver<(Option<SubscriptionStatus>, Option<UsageStats>)>>,
    // WebSocket client for real-time updates
    ws_client: Option<Arc<WebSocketClient>>,
    ws_initialized: bool,
    ws_subscription_rx: Option<std::sync::mpsc::Receiver<SubscriptionStatus>>,
    ws_usage_rx: Option<std::sync::mpsc::Receiver<UsageStats>>,
}

#[derive(Debug, Clone)]
enum AuthResult {
    Success { email: String },
    Error(String),
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Initialize WebSocket for real-time updates if authenticated
        if self.is_authenticated && !self.ws_initialized {
            self.initialize_websocket(ctx);
        }
        
        // Check for real-time subscription updates from WebSocket
        if let Some(ref rx) = self.ws_subscription_rx {
            if let Ok(status) = rx.try_recv() {
                info!("Received real-time subscription update in UI");
                self.subscription_status = Some(status);
                self.subscription_loading = false;
            }
        }
        
        // Check for real-time usage updates from WebSocket
        if let Some(ref rx) = self.ws_usage_rx {
            if let Ok(stats) = rx.try_recv() {
                info!("Received real-time usage update in UI");
                self.usage_stats = Some(stats);
            }
        }
        
        // Check if window just gained focus after being in background (user might have upgraded in browser)
        static mut LAST_FOCUS_STATE: bool = false;
        let is_focused = ctx.input(|i| i.focused);
        let gained_focus = unsafe {
            let was_unfocused = !LAST_FOCUS_STATE;
            LAST_FOCUS_STATE = is_focused;
            is_focused && was_unfocused
        };
        
        if gained_focus && self.is_authenticated && !self.subscription_loading && self.api_client.is_some() {
            // Window regained focus - user might have upgraded in browser
            // Only refresh if it's been at least 5 seconds since last fetch to avoid rapid refreshes
            static mut LAST_FETCH_TIME: Option<std::time::Instant> = None;
            let should_refresh = unsafe {
                match LAST_FETCH_TIME {
                    None => true,
                    Some(last) => last.elapsed() > std::time::Duration::from_secs(5),
                }
            };
            
            if should_refresh {
                info!("Window regained focus, refreshing subscription status");
                self.fetch_subscription_status(ctx);
                unsafe {
                    LAST_FETCH_TIME = Some(std::time::Instant::now());
                }
            }
        }
        
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
                    info!("Checking auth status on window show, auth_manager exists: {}", self.auth_manager.is_some());
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
                info!("Auth status check result: authenticated={}, email={:?}", is_auth, email);
                self.is_authenticated = is_auth;
                self.user_email = email.clone();
                self.auth_check_rx = None; // Clear after receiving
                // Fetch subscription status if authenticated
                if is_auth && self.api_client.is_some() {
                    info!("User is authenticated with email: {:?}, fetching subscription automatically", email);
                    self.fetch_subscription_status(ctx);
                } else if is_auth {
                    info!("User is authenticated but no API client available");
                }
                ctx.request_repaint();
            }
        }
        
        // Check for subscription status results
        if let Some(ref rx) = self.subscription_rx {
            if let Ok((subscription, usage)) = rx.try_recv() {
                info!("Received subscription response - subscription: {}, usage: {}", 
                      subscription.is_some(), usage.is_some());
                self.subscription_loading = false;
                self.subscription_status = subscription;
                self.usage_stats = usage;
                self.subscription_rx = None;
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
                        // Reset WebSocket initialized flag to trigger reconnection with new auth
                        self.ws_initialized = false;
                        // Fetch subscription status after successful auth
                        self.fetch_subscription_status(ctx);
                    }
                    AuthResult::Error(err) => {
                        self.auth_error = Some(err);
                    }
                }
                self.auth_result_rx = None;
                ctx.request_repaint();
            }
        }
        
        // If auth is loading, periodically check auth status (but not too often)
        // This handles edge cases where browser auth succeeds but callback fails
        if self.auth_is_loading && self.auth_result_rx.is_some() {
            static mut LAST_AUTH_LOADING_CHECK: Option<std::time::Instant> = None;
            let should_check_auth_status = unsafe {
                match LAST_AUTH_LOADING_CHECK {
                    None => {
                        LAST_AUTH_LOADING_CHECK = Some(std::time::Instant::now());
                        false // Don't check immediately, wait for OAuth callback first
                    }
                    Some(last) => {
                        // Only check every 5 seconds to reduce server load
                        if last.elapsed() > std::time::Duration::from_secs(5) {
                            LAST_AUTH_LOADING_CHECK = Some(std::time::Instant::now());
                            true
                        } else {
                            false
                        }
                    }
                }
            };
            
            if should_check_auth_status {
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
                    
                    // Check immediately without blocking
                    if let Ok((is_auth, email)) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
                        if is_auth && email.is_some() {
                            // Auth succeeded! Update state
                            self.is_authenticated = true;
                            self.user_email = email;
                            self.auth_is_loading = false;
                            self.auth_error = None;
                            self.auth_result_rx = None;
                            info!("Browser auth detected as successful via polling");
                            ctx.request_repaint();
                        }
                    }
                }
            }
        }
        
        // No need for periodic auth checks - we already check when:
        // 1. Window is shown (line 423-443)
        // 2. Auth flow completes (line 447-520)
        // 3. User explicitly logs in/out
        // This prevents unnecessary server load
        
        // Request repaint to keep checking for commands
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        
        // Only show UI if visible
        if !self.visible {
            return;
        }
        
        // Action flag to avoid borrow checker issues
        let mut should_logout = false;
        
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
                                if ui.button("🔃 Refresh Status").clicked() {
                                    self.fetch_subscription_status(ui.ctx());
                                }
                            });
                        });
                    });
                ui.add_space(10.0);
                
                // Show subscription status
                if let Some(ref subscription) = self.subscription_status {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(35, 35, 45))
                        .rounding(egui::Rounding::same(5.0))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                // Subscription tier
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("📦").size(16.0));
                                    ui.label(egui::RichText::new(format!("Plan: {}", subscription.tier.name))
                                        .color(egui::Color32::from_rgb(150, 200, 255))
                                        .size(14.0));
                                    if subscription.is_active() {
                                        ui.label(egui::RichText::new("Active").color(egui::Color32::from_rgb(100, 255, 100)).size(12.0));
                                    }
                                    
                                    // Add upgrade button for non-lifetime plans
                                    if subscription.tier.id != "lifetime" {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.button("⬆ Upgrade").clicked() {
                                                // Open web dashboard for subscription management
                                                let dashboard_url = format!("{}/dashboard/billing", 
                                                    self.settings.lock().unwrap().cloud_api_url
                                                        .replace("/api", "")
                                                        .replace(":8080", ":3000")); // Handle local dev
                                                
                                                if let Err(e) = webbrowser::open(&dashboard_url) {
                                                    error!("Failed to open browser: {}", e);
                                                } else {
                                                    info!("Opened dashboard URL: {}", dashboard_url);
                                                }
                                            }
                                        });
                                    }
                                });
                                
                                // Usage stats
                                if let Some(ref usage) = self.usage_stats {
                                    ui.add_space(8.0);
                                    
                                    // Saves usage
                                    ui.horizontal(|ui| {
                                        ui.label("Saves:");
                                        let saves_pct = usage.saves_percentage();
                                        let color = if saves_pct > 90.0 {
                                            egui::Color32::from_rgb(255, 100, 100)
                                        } else if saves_pct > 75.0 {
                                            egui::Color32::from_rgb(255, 200, 100)
                                        } else {
                                            egui::Color32::from_rgb(100, 255, 100)
                                        };
                                        ui.label(egui::RichText::new(format!("{}/{} ({:.0}%)", 
                                            usage.saves_count, usage.saves_limit, saves_pct))
                                            .color(color)
                                            .size(12.0));
                                    });
                                    
                                    // Storage usage
                                    ui.horizontal(|ui| {
                                        ui.label("Storage:");
                                        let storage_pct = usage.storage_percentage();
                                        let storage_gb = usage.storage_bytes as f64 / 1_073_741_824.0;
                                        let storage_limit_gb = usage.storage_limit_bytes as f64 / 1_073_741_824.0;
                                        let color = if storage_pct > 90.0 {
                                            egui::Color32::from_rgb(255, 100, 100)
                                        } else if storage_pct > 75.0 {
                                            egui::Color32::from_rgb(255, 200, 100)
                                        } else {
                                            egui::Color32::from_rgb(100, 255, 100)
                                        };
                                        ui.label(egui::RichText::new(format!("{:.2}/{:.0} GB ({:.0}%)", 
                                            storage_gb, storage_limit_gb, storage_pct))
                                            .color(color)
                                            .size(12.0));
                                    });
                                    
                                    // Devices usage
                                    ui.horizontal(|ui| {
                                        ui.label("Devices:");
                                        let devices_pct = if usage.devices_limit > 0 {
                                            (usage.devices_count as f32 / usage.devices_limit as f32) * 100.0
                                        } else {
                                            0.0
                                        };
                                        let color = if devices_pct >= 100.0 {
                                            egui::Color32::from_rgb(255, 100, 100)
                                        } else if devices_pct > 80.0 {
                                            egui::Color32::from_rgb(255, 200, 100)
                                        } else {
                                            egui::Color32::from_rgb(100, 255, 100)
                                        };
                                        ui.label(egui::RichText::new(format!("{}/{}", 
                                            usage.devices_count, usage.devices_limit))
                                            .color(color)
                                            .size(12.0));
                                    });
                                    
                                    // Warning if near limits
                                    if usage.is_near_limit() {
                                        ui.add_space(5.0);
                                        ui.label(egui::RichText::new("⚠ Approaching limits")
                                            .color(egui::Color32::from_rgb(255, 200, 100))
                                            .size(11.0));
                                    }
                                }
                            });
                        });
                    ui.add_space(10.0);
                } else if self.subscription_loading {
                    ui.label(egui::RichText::new("⏳ Loading subscription status...")
                        .color(egui::Color32::from_rgb(150, 150, 150))
                        .size(12.0));
                    ui.add_space(10.0);
                }
                
                // Cloud sync checkbox for authenticated users
                {
                    let mut settings = self.settings.lock().unwrap();
                    ui.checkbox(&mut settings.cloud_sync_enabled, "Enable cloud sync");
                    cloud_sync_enabled = settings.cloud_sync_enabled;
                }
            } else {
                // Not authenticated - show browser auth UI
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(40, 45, 40))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(16.0))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            // Icon and heading
                            ui.label(egui::RichText::new("🔐").size(32.0));
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("Sign in to enable cloud sync").size(16.0).strong());
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("Securely sync your saves across all devices").size(12.0).color(egui::Color32::from_rgb(150, 150, 150)));
                            
                            ui.add_space(16.0);
                            
                            // Browser auth button
                            ui.add_enabled_ui(!self.auth_is_loading, |ui| {
                                let button_text = if self.auth_is_loading {
                                    "⏳ Waiting for authentication..."
                                } else {
                                    "🌐 Sign in with Browser"
                                };
                                
                                let button = egui::Button::new(
                                    egui::RichText::new(button_text).size(14.0)
                                )
                                .min_size(egui::Vec2::new(200.0, 36.0))
                                .rounding(egui::Rounding::same(6.0));
                                
                                if ui.add(button).clicked() {
                                    self.start_browser_auth();
                                }
                            });
                            
                            // Show loading message if authenticating
                            if self.auth_is_loading {
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("⚠ Complete authentication in your browser").size(12.0).color(egui::Color32::from_rgb(255, 200, 100)));
                                ui.label(egui::RichText::new("This window will automatically update when done").size(11.0).color(egui::Color32::from_rgb(150, 150, 150)));
                            }
                            
                            // Error message
                            if let Some(ref error) = self.auth_error {
                                ui.add_space(8.0);
                                ui.colored_label(egui::Color32::from_rgb(255, 100, 100), format!("⚠ {}", error));
                            }
                            
                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(8.0);
                            
                            // Benefits list
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new("✅ Secure OAuth 2.0 authentication").size(11.0).color(egui::Color32::from_rgb(130, 130, 130)));
                                ui.label(egui::RichText::new("✅ Two-factor authentication support").size(11.0).color(egui::Color32::from_rgb(130, 130, 130)));
                                ui.label(egui::RichText::new("✅ Sign in with Google or GitHub").size(11.0).color(egui::Color32::from_rgb(130, 130, 130)));
                            });
                        });
                    });
                ui.add_space(10.0);
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
    }
}

impl SettingsApp {
    fn initialize_websocket(&mut self, ctx: &egui::Context) {
        if self.ws_initialized || !self.is_authenticated {
            return;
        }
        
        info!("Initializing WebSocket connection for real-time updates");
        
        // Get API URL from settings
        let api_url = self.settings.lock().unwrap().cloud_api_url.clone();
        
        // Create WebSocket event channel
        let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
        
        // Create WebSocket client
        let ws_client = Arc::new(WebSocketClient::new(api_url.clone(), event_tx));
        let ws_client_clone = ws_client.clone();
        
        // Get auth token if available
        if let Some(ref auth_manager) = self.auth_manager {
            let auth_manager_clone = auth_manager.clone();
            let ws_client_for_auth = ws_client.clone();
            let api_url_clone = api_url.clone();
            
            // Set up WebSocket with auth token
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    if let Some(token) = auth_manager_clone.get_access_token().await {
                        // Create a new mutable WebSocket client with the token
                        let mut ws = WebSocketClient::new(api_url_clone.clone(), tokio::sync::mpsc::unbounded_channel().0);
                        ws.set_token(token).await;
                        
                        // Connect to WebSocket
                        if let Err(e) = ws_client_for_auth.connect().await {
                            error!("Failed to connect WebSocket: {}", e);
                        } else {
                            info!("WebSocket connected successfully");
                            
                            // Start listening for messages
                            let ws_listener = ws_client_for_auth.clone();
                            tokio::spawn(async move {
                                ws_listener.start_listening().await;
                            });
                        }
                    }
                });
            });
        }
        
        // Create channels to update the UI from callbacks
        let (subscription_tx, subscription_rx) = std::sync::mpsc::channel::<SubscriptionStatus>();
        let (usage_tx, usage_rx) = std::sync::mpsc::channel::<UsageStats>();
        
        // Store receivers so we can poll them in the update loop
        self.ws_subscription_rx = Some(subscription_rx);
        self.ws_usage_rx = Some(usage_rx);
        
        // Register subscription update callback
        let ctx_clone = ctx.clone();
        let ws_handler = ws_client.event_handler();
        let subscription_callback = {
            let ctx = ctx_clone.clone();
            let tx = subscription_tx.clone();
            move |status: SubscriptionStatus| {
                info!("Received real-time subscription update: {:?}", status);
                let _ = tx.send(status);
                ctx.request_repaint();
            }
        };
        
        // Register usage update callback
        let ctx_clone2 = ctx.clone();
        let usage_callback = {
            let ctx = ctx_clone2.clone();
            let tx = usage_tx.clone();
            move |stats: UsageStats| {
                info!("Received real-time usage update: {:?}", stats);
                let _ = tx.send(stats);
                ctx.request_repaint();
            }
        };
        
        // Register device added callback
        let ctx_clone3 = ctx.clone();
        let device_added_callback = {
            let ctx = ctx_clone3.clone();
            move |device_id: String, device_name: String| {
                info!("Device added: {} ({})", device_name, device_id);
                // Show notification to user
                if let Err(e) = crate::ui::notifications::show_notification(
                    "Device Added",
                    &format!("New device '{}' has been added to your account", device_name),
                    crate::ui::notifications::NotificationType::Info,
                ) {
                    error!("Failed to show device added notification: {}", e);
                }
                ctx.request_repaint();
            }
        };
        
        // Register device removed callback
        let ctx_clone4 = ctx.clone();
        let device_removed_callback = {
            let ctx = ctx_clone4.clone();
            move |device_id: String, device_name: String| {
                info!("Device removed: {} ({})", device_name, device_id);
                // Show notification to user
                if let Err(e) = crate::ui::notifications::show_notification(
                    "Device Removed",
                    &format!("Device '{}' has been removed from your account", device_name),
                    crate::ui::notifications::NotificationType::Warning,
                ) {
                    error!("Failed to show device removed notification: {}", e);
                }
                ctx.request_repaint();
            }
        };
        
        // Register warning callback (for storage/save limits)
        let ctx_clone5 = ctx.clone();
        let warning_callback = {
            let ctx = ctx_clone5.clone();
            move |message: String| {
                info!("Received warning: {}", message);
                // Show warning notification to user
                if let Err(e) = crate::ui::notifications::show_notification(
                    "Storage Warning",
                    &message,
                    crate::ui::notifications::NotificationType::Warning,
                ) {
                    error!("Failed to show warning notification: {}", e);
                }
                ctx.request_repaint();
            }
        };
        
        // Store callbacks using a separate thread to avoid blocking
        let ws_handler_clone = ws_handler.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                ws_handler_clone.on_subscription_update(subscription_callback).await;
                ws_handler_clone.on_usage_update(usage_callback).await;
                ws_handler_clone.on_device_added(device_added_callback).await;
                ws_handler_clone.on_device_removed(device_removed_callback).await;
                ws_handler_clone.on_warning(warning_callback).await;
            });
        });
        
        self.ws_client = Some(ws_client_clone);
        self.ws_initialized = true;
        
        info!("WebSocket initialization complete");
    }
    
    fn fetch_subscription_status(&mut self, ctx: &egui::Context) {
        info!("fetch_subscription_status called, api_client exists: {}", self.api_client.is_some());
        if let Some(ref api_client) = self.api_client {
            if self.subscription_loading {
                info!("Already loading subscription, skipping");
                return; // Already loading
            }
            
            info!("Starting subscription fetch");
            self.subscription_loading = true;
            let api_client_clone = api_client.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let mut subscription = None;
                    let mut usage = None;
                    
                    // Fetch subscription status with retry on auth refresh
                    for attempt in 0..2 {
                        match api_client_clone.get_subscription_status().await {
                            Ok(status) => {
                                subscription = Some(status);
                                break;
                            },
                            Err(e) => {
                                let error_str = e.to_string();
                                if error_str.contains("Authentication refreshed") && attempt == 0 {
                                    // First attempt failed with auth refresh, retry
                                    info!("Retrying subscription fetch after auth refresh");
                                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                    continue;
                                }
                                error!("Failed to fetch subscription status: {}", e);
                                break;
                            }
                        }
                    }
                    
                    // Fetch usage stats with retry on auth refresh
                    for attempt in 0..2 {
                        match api_client_clone.get_usage_stats().await {
                            Ok(stats) => {
                                usage = Some(stats);
                                break;
                            },
                            Err(e) => {
                                let error_str = e.to_string();
                                if error_str.contains("Authentication refreshed") && attempt == 0 {
                                    // First attempt failed with auth refresh, retry
                                    info!("Retrying usage stats fetch after auth refresh");
                                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                    continue;
                                }
                                error!("Failed to fetch usage stats: {}", e);
                                break;
                            }
                        }
                    }
                    
                    let _ = tx.send((subscription, usage));
                });
            });
            
            self.subscription_rx = Some(rx);
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    
    fn perform_logout(&mut self, ctx: &egui::Context) {
        // Disconnect WebSocket if connected
        if let Some(ref ws_client) = self.ws_client {
            let ws_client_clone = ws_client.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    if let Err(e) = ws_client_clone.disconnect().await {
                        error!("Failed to disconnect WebSocket: {}", e);
                    } else {
                        info!("WebSocket disconnected");
                    }
                });
            });
            self.ws_client = None;
            self.ws_initialized = false;
            self.ws_subscription_rx = None;
            self.ws_usage_rx = None;
        }
        
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
            self.subscription_status = None;
            self.usage_stats = None;
        }
        ctx.request_repaint();
    }
    
    fn start_browser_auth(&mut self) {
        use crate::auth::BrowserOAuth;
        
        // Set loading state
        self.auth_is_loading = true;
        self.auth_error = None;
        
        // Get API URL from settings
        let api_url = {
            let settings = self.settings.lock().unwrap();
            settings.cloud_api_url.clone()
        };
        
        info!("Starting browser OAuth flow with API URL: {}", api_url);
        
        // Create channel for result
        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_result_rx = Some(rx);
        
        // Get auth manager reference
        let auth_manager = self.auth_manager.clone();
        
        // Start OAuth flow in background thread
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let oauth_client = BrowserOAuth::new(api_url);
                
                match oauth_client.authenticate().await {
                    Ok(token_response) => {
                        info!("OAuth flow successful, got tokens for user: {}", token_response.user.email);
                        
                        // Save tokens if we have an auth manager
                        if let Some(auth_mgr) = auth_manager {
                            if let Err(e) = auth_mgr.save_tokens(
                                token_response.access_token.clone(),
                                token_response.refresh_token.clone(),
                                token_response.user.clone()
                            ).await {
                                error!("Failed to save OAuth tokens: {}", e);
                                let _ = tx.send(AuthResult::Error(
                                    format!("Failed to save authentication: {}", e)
                                ));
                                return;
                            }
                        }
                        
                        let _ = tx.send(AuthResult::Success {
                            email: token_response.user.email
                        });
                    }
                    Err(e) => {
                        error!("OAuth flow failed: {}", e);
                        let error_msg = if e.to_string().contains("timeout") {
                            "Authentication timed out. Please try again.".to_string()
                        } else if e.to_string().contains("cancel") {
                            "Authentication cancelled.".to_string()
                        } else {
                            format!("Authentication failed: {}", e)
                        };
                        let _ = tx.send(AuthResult::Error(error_msg));
                    }
                }
            });
        });
    }
}