use anyhow::Result;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIconBuilder, TrayIconEvent, TrayIcon,
};
use tracing::{debug, info};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use notify_rust::{Notification, Timeout};

// GTK initialization on Linux
#[cfg(target_os = "linux")]
extern crate gtk;
#[cfg(target_os = "linux")]
extern crate glib;

#[derive(Debug, Clone)]
pub enum TrayMessage {
    EmulatorDetected(String),
    EmulatorStopped,
    GameDetected(String),
    SaveDetected(String),
    UpdateStatus(String),
    ManualSaveRequested,
    OpenSettings,
    OpenDashboard,
    HotkeyChanged(Option<String>),
    // Cloud sync messages
    SyncStarted,
    SyncCompleted { uploaded: usize, downloaded: usize },
    SyncFailed(String),
    CloudAuthChanged { is_authenticated: bool, email: Option<String> },
}

// Control messages for the tray thread
#[derive(Debug, Clone)]
pub enum TrayControl {
    UpdateStatus(String),
    ShowNotification(String, String),
    Exit,
}

pub struct SystemTray {
    status: Arc<Mutex<String>>,
    sender: mpsc::Sender<TrayMessage>,
    control_sender: mpsc::Sender<TrayControl>,
}

impl SystemTray {
    pub fn new() -> Result<(Self, mpsc::Receiver<TrayMessage>)> {
        let (sender, receiver) = mpsc::channel(100);
        let (control_sender, control_receiver) = mpsc::channel(100);
        
        // Clone for the tray thread
        let sender_clone = sender.clone();
        
        // Create the tray icon in a platform-specific way
        #[cfg(target_os = "linux")]
        {
            // On Linux, create tray in a dedicated thread with GTK main loop
            std::thread::spawn(move || {
                if let Err(e) = gtk::init() {
                    error!("Failed to initialize GTK: {}", e);
                    return;
                }
                
                match Self::create_tray_icon_linux(sender_clone, control_receiver) {
                    Ok(_) => {
                        info!("GTK main loop ended");
                    }
                    Err(e) => {
                        error!("Failed to create tray icon: {}", e);
                    }
                }
            });
        }
        
        #[cfg(not(target_os = "linux"))]
        {
            // On Windows/macOS, create tray on current thread
            std::thread::spawn(move || {
                match Self::create_tray_icon(sender_clone) {
                    Ok(_tray_icon) => {
                        info!("Tray icon created successfully");
                        // Keep the tray icon alive by holding it in this thread
                        loop {
                            if let Some(msg) = control_receiver.blocking_recv() {
                                match msg {
                                    TrayControl::UpdateStatus(status) => {
                                        info!("Updating tray status: {}", status);
                                    }
                                    TrayControl::ShowNotification(title, message) => {
                                        Self::show_notification_internal(&title, &message);
                                    }
                                    TrayControl::Exit => {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to create tray icon: {}", e);
                    }
                }
            });
        }
        
        let tray = Self {
            status: Arc::new(Mutex::new("Starting...".to_string())),
            sender,
            control_sender,
        };
        
        Ok((tray, receiver))
    }
    
    #[cfg(target_os = "linux")]
    fn create_tray_icon_linux(
        event_sender: mpsc::Sender<TrayMessage>,
        mut control_receiver: mpsc::Receiver<TrayControl>,
    ) -> Result<()> {
        info!("Creating tray icon and menu for Linux");
        
        // Create menu
        let menu = Menu::new();
        
        // Status item (disabled, just for display)
        let status_item = MenuItem::new("Status: Monitoring", true, None);
        menu.append(&status_item)?;
        
        // Separator
        menu.append(&PredefinedMenuItem::separator())?;
        
        // Save Now item
        let save_now_item = MenuItem::new("Save Now", true, None);
        menu.append(&save_now_item)?;
        
        // Separator
        menu.append(&PredefinedMenuItem::separator())?;
        
        // Dashboard item
        let dashboard_item = MenuItem::new("Dashboard", true, None);
        menu.append(&dashboard_item)?;
        
        // Settings item
        let settings_item = MenuItem::new("Settings", true, None);
        menu.append(&settings_item)?;
        
        // About item
        let about_item = MenuItem::new("About", true, None);
        menu.append(&about_item)?;
        
        // Separator
        menu.append(&PredefinedMenuItem::separator())?;
        
        // Exit item
        let exit_item = MenuItem::new("Exit", true, None);
        menu.append(&exit_item)?;
        
        // Store menu item IDs for the event handler
        let exit_id = exit_item.id().clone();
        let save_now_id = save_now_item.id().clone();
        let dashboard_id = dashboard_item.id().clone();
        let settings_id = settings_item.id().clone();
        let about_id = about_item.id().clone();
        
        // Load or create icon
        let icon = Self::load_icon()?;
        
        // Create tray icon
        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Retrosave - Monitoring")
            .with_icon(icon)
            .build()?;
        
        info!("Tray icon created successfully on Linux");
        
        // Keep tray icon alive
        let tray_icon = Arc::new(Mutex::new(Some(tray_icon)));
        let tray_icon_clone = tray_icon.clone();
        
        // Handle all events in GTK idle callback
        glib::idle_add_local(move || {
            // Check for control messages
            if let Ok(msg) = control_receiver.try_recv() {
                match msg {
                    TrayControl::UpdateStatus(status) => {
                        info!("Updating tray status: {}", status);
                        // TODO: Update tooltip or menu item
                    }
                    TrayControl::ShowNotification(title, message) => {
                        Self::show_notification_internal(&title, &message);
                    }
                    TrayControl::Exit => {
                        if let Ok(mut tray_guard) = tray_icon_clone.lock() {
                            *tray_guard = None; // Drop the tray icon
                        }
                        gtk::main_quit();
                        return glib::ControlFlow::Break;
                    }
                }
            }
            
            // Check for menu events
            if let Ok(event) = MenuEvent::receiver().try_recv() {
                info!("Menu event received: {:?}", event.id);
                if event.id == exit_id {
                    info!("Exit requested from tray menu");
                    gtk::main_quit();
                    std::process::exit(0);
                } else if event.id == save_now_id {
                    info!("Manual save requested from tray menu");
                    let _ = event_sender.try_send(TrayMessage::ManualSaveRequested);
                } else if event.id == dashboard_id {
                    info!("Dashboard clicked");
                    let _ = event_sender.try_send(TrayMessage::OpenDashboard);
                } else if event.id == settings_id {
                    info!("Settings clicked");
                    let _ = event_sender.try_send(TrayMessage::OpenSettings);
                } else if event.id == about_id {
                    info!("About clicked");
                    // TODO: Show about dialog
                }
            }
            
            // Check for tray icon events (like clicks)
            if let Ok(event) = TrayIconEvent::receiver().try_recv() {
                info!("Tray event received: {:?}", event);
                match event {
                    TrayIconEvent::Click { .. } => {
                        info!("Tray icon clicked");
                    }
                    TrayIconEvent::DoubleClick { .. } => {
                        info!("Tray icon double-clicked");
                        // TODO: Show main window when implemented
                    }
                    _ => {
                        info!("Other tray event: {:?}", event);
                    }
                }
            }
            
            glib::ControlFlow::Continue
        });
        
        // Run GTK main loop - this blocks until quit
        gtk::main();
        
        Ok(())
    }
    
    fn create_tray_icon(event_sender: mpsc::Sender<TrayMessage>) -> Result<TrayIcon> {
        info!("Creating tray icon and menu");
        
        // Create menu
        let menu = Menu::new();
        
        // Status item (disabled, just for display)
        let status_item = MenuItem::new("Status: Monitoring", true, None);
        menu.append(&status_item)?;
        
        // Separator
        menu.append(&PredefinedMenuItem::separator())?;
        
        // Save Now item
        let save_now_item = MenuItem::new("Save Now", true, None);
        menu.append(&save_now_item)?;
        
        // Separator
        menu.append(&PredefinedMenuItem::separator())?;
        
        // Dashboard item
        let dashboard_item = MenuItem::new("Dashboard", true, None);
        menu.append(&dashboard_item)?;
        
        // Settings item
        let settings_item = MenuItem::new("Settings", true, None);
        menu.append(&settings_item)?;
        
        // About item
        let about_item = MenuItem::new("About", true, None);
        menu.append(&about_item)?;
        
        // Separator
        menu.append(&PredefinedMenuItem::separator())?;
        
        // Exit item
        let exit_item = MenuItem::new("Exit", true, None);
        menu.append(&exit_item)?;
        
        // Store menu item IDs for the event handler
        let exit_id = exit_item.id().clone();
        let save_now_id = save_now_item.id().clone();
        let dashboard_id = dashboard_item.id().clone();
        let settings_id = settings_item.id().clone();
        let about_id = about_item.id().clone();
        
        // Load or create icon
        let icon = Self::load_icon()?;
        
        // Create tray icon
        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Retrosave - Monitoring")
            .with_icon(icon)
            .build()?;
        
        // Handle menu events in a separate thread (non-Linux platforms)
        std::thread::spawn(move || {
            info!("Menu event handler thread started");
            let menu_channel = MenuEvent::receiver();
            let tray_channel = TrayIconEvent::receiver();
            
            loop {
                // Check for menu events
                if let Ok(event) = menu_channel.try_recv() {
                    info!("Menu event received: {:?}", event.id);
                    if event.id == exit_id {
                        info!("Exit requested from tray menu");
                        std::process::exit(0);
                    } else if event.id == save_now_id {
                        info!("Manual save requested from tray menu");
                        let _ = event_sender.blocking_send(TrayMessage::ManualSaveRequested);
                    } else if event.id == dashboard_id {
                        info!("Dashboard clicked");
                        let _ = event_sender.blocking_send(TrayMessage::OpenDashboard);
                    } else if event.id == settings_id {
                        info!("Settings clicked");
                        let _ = event_sender.blocking_send(TrayMessage::OpenSettings);
                    } else if event.id == about_id {
                        info!("About clicked");
                        // TODO: Show about dialog
                    }
                }
                
                // Check for tray icon events (like clicks)
                if let Ok(event) = tray_channel.try_recv() {
                    info!("Tray event received: {:?}", event);
                    match event {
                        TrayIconEvent::Click { .. } => {
                            info!("Tray icon clicked");
                        }
                        TrayIconEvent::DoubleClick { .. } => {
                            info!("Tray icon double-clicked");
                            // TODO: Show main window when implemented
                        }
                        _ => {
                            info!("Other tray event: {:?}", event);
                        }
                    }
                }
                
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
        
        Ok(tray_icon)
    }
    
    pub fn update_status(&self, status: &str) {
        if let Ok(mut s) = self.status.lock() {
            *s = status.to_string();
        }
        let _ = self.control_sender.try_send(TrayControl::UpdateStatus(status.to_string()));
    }
    
    pub async fn send_message(&self, message: TrayMessage) -> Result<()> {
        self.sender.send(message).await?;
        Ok(())
    }
    
    pub fn show_notification(&self, title: &str, message: &str) {
        let _ = self.control_sender.try_send(
            TrayControl::ShowNotification(title.to_string(), message.to_string())
        );
    }
    
    fn show_notification_internal(title: &str, message: &str) {
        info!("Notification: {} - {}", title, message);
        
        // Show actual desktop notification
        if let Err(e) = Notification::new()
            .summary(title)
            .body(message)
            .appname("Retrosave")
            .icon("dialog-information")
            .timeout(Timeout::Milliseconds(5000))
            .show()
        {
            debug!("Failed to show desktop notification: {}", e);
        }
    }
    
    fn load_icon() -> Result<tray_icon::Icon> {
        // Try to load the actual icon file first
        let icon_paths = [
            "/home/eralp/Projects/retrosave/client/assets/icon-32.png",
            "/home/eralp/Projects/retrosave/client/assets/icon.svg",
            "assets/icon-32.png",
            "assets/icon.svg",
        ];
        
        for path in &icon_paths {
            if std::path::Path::new(path).exists() {
                match Self::load_icon_from_file(path) {
                    Ok(icon) => return Ok(icon),
                    Err(e) => debug!("Failed to load icon from {}: {}", path, e),
                }
            }
        }
        
        // Fallback to programmatic icon
        debug!("Using programmatic icon as fallback");
        let rgba = Self::create_default_icon();
        let icon = tray_icon::Icon::from_rgba(rgba, 32, 32)?;
        Ok(icon)
    }
    
    fn load_icon_from_file(path: &str) -> Result<tray_icon::Icon> {
        if path.ends_with(".png") {
            let image = image::open(path)?;
            let rgba = image.to_rgba8();
            let (width, height) = rgba.dimensions();
            let icon = tray_icon::Icon::from_rgba(rgba.into_raw(), width, height)?;
            Ok(icon)
        } else {
            // For SVG or other formats, fallback to default
            Err(anyhow::anyhow!("Unsupported icon format"))
        }
    }
    
    fn create_default_icon() -> Vec<u8> {
        // Create a simple 32x32 green square icon
        let size = 32 * 32 * 4; // width * height * 4 (RGBA)
        let mut rgba = vec![0u8; size];
        
        for i in (0..size).step_by(4) {
            rgba[i] = 34;      // R
            rgba[i + 1] = 139;  // G  (green color)
            rgba[i + 2] = 34;   // B
            rgba[i + 3] = 255;  // A (fully opaque)
        }
        
        rgba
    }
}

// Add missing dependencies to imports
use tracing::error;