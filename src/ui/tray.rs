use anyhow::Result;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIconBuilder, TrayIconEvent,
};
use tracing::{debug, error, info};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum TrayMessage {
    EmulatorDetected(String),
    EmulatorStopped,
    GameDetected(String),
    SaveDetected(String),
    UpdateStatus(String),
}

#[derive(Clone)]
pub struct SystemTray {
    status: Arc<Mutex<String>>,
    sender: mpsc::Sender<TrayMessage>,
}

impl SystemTray {
    pub fn new() -> Result<(Self, mpsc::Receiver<TrayMessage>)> {
        let (sender, receiver) = mpsc::channel(100);
        
        let tray = Self {
            status: Arc::new(Mutex::new("Starting...".to_string())),
            sender,
        };
        
        Ok((tray, receiver))
    }
    
    pub fn init(&self) -> Result<()> {
        info!("Initializing system tray");
        
        // Create menu
        let menu = Menu::new();
        
        // Status item (disabled, just for display)
        let status_item = MenuItem::new("Status: Monitoring", true, None);
        menu.append(&status_item)?;
        
        // Separator
        menu.append(&PredefinedMenuItem::separator())?;
        
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
        let settings_id = settings_item.id().clone();
        let about_id = about_item.id().clone();
        
        // Create tray icon
        let icon = Self::load_icon()?;
        let _tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Retrosave - Monitoring")
            .with_icon(icon)
            .build()?;
        
        // Handle menu events in a separate task
        std::thread::spawn(move || {
            let menu_channel = MenuEvent::receiver();
            let tray_channel = TrayIconEvent::receiver();
            
            loop {
                // Check for menu events
                if let Ok(event) = menu_channel.try_recv() {
                    if event.id == exit_id {
                        info!("Exit requested from tray menu");
                        std::process::exit(0);
                    } else if event.id == settings_id {
                        info!("Settings clicked - not implemented yet");
                    } else if event.id == about_id {
                        info!("About clicked");
                        // TODO: Show about dialog
                    }
                }
                
                // Check for tray icon events (like clicks)
                if let Ok(event) = tray_channel.try_recv() {
                    match event {
                        TrayIconEvent::Click { .. } => {
                            debug!("Tray icon clicked");
                        }
                        TrayIconEvent::DoubleClick { .. } => {
                            debug!("Tray icon double-clicked");
                            // TODO: Show main window when implemented
                        }
                        _ => {}
                    }
                }
                
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
        
        // Update initial status
        self.update_status("Ready - Monitoring for emulators");
        
        Ok(())
    }
    
    pub fn update_status(&self, status: &str) {
        if let Ok(mut s) = self.status.lock() {
            *s = status.to_string();
            info!("Tray status updated: {}", status);
            // TODO: Update menu item text
            // TODO: Update tooltip
        }
    }
    
    pub async fn send_message(&self, message: TrayMessage) -> Result<()> {
        self.sender.send(message).await?;
        Ok(())
    }
    
    fn load_icon() -> Result<tray_icon::Icon> {
        // For now, create a simple colored icon programmatically
        // In production, load from assets/icons/
        let rgba = Self::create_default_icon();
        let icon = tray_icon::Icon::from_rgba(rgba, 32, 32)?;
        Ok(icon)
    }
    
    fn create_default_icon() -> Vec<u8> {
        // Create a simple 32x32 green square icon
        // In production, use proper icon files
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
    
    pub fn show_notification(&self, title: &str, message: &str) {
        // For Linux, we could use notify-rust
        // For Windows, we could use windows-rs
        // For now, just log
        info!("Notification: {} - {}", title, message);
        
        // TODO: Implement actual notifications
        #[cfg(target_os = "linux")]
        {
            // Use notify-rust or similar
        }
        
        #[cfg(target_os = "windows")]
        {
            // Use windows notifications
        }
    }
}