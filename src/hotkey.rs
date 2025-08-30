use anyhow::Result;
use global_hotkey::{
    GlobalHotKeyManager, 
    hotkey::{HotKey, Code, Modifiers},
    GlobalHotKeyEvent
};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{info, error, debug};

#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    SaveNow,
}

pub struct HotkeyManager {
    manager: Arc<Mutex<GlobalHotKeyManager>>,
    current_hotkey: Arc<Mutex<Option<HotKey>>>,
    event_sender: mpsc::Sender<HotkeyEvent>,
}

impl HotkeyManager {
    pub fn new(event_sender: mpsc::Sender<HotkeyEvent>) -> Result<Self> {
        let manager = GlobalHotKeyManager::new()?;
        
        Ok(Self {
            manager: Arc::new(Mutex::new(manager)),
            current_hotkey: Arc::new(Mutex::new(None)),
            event_sender,
        })
    }
    
    pub fn set_save_hotkey(&self, hotkey_str: Option<String>) -> Result<()> {
        let manager = self.manager.lock().unwrap();
        let mut current = self.current_hotkey.lock().unwrap();
        
        // Unregister previous hotkey if exists
        if let Some(old_hotkey) = current.take() {
            manager.unregister(old_hotkey)?;
            debug!("Unregistered previous hotkey");
        }
        
        // Register new hotkey if provided
        if let Some(hotkey_str) = hotkey_str {
            if let Ok(hotkey) = Self::parse_hotkey(&hotkey_str) {
                manager.register(hotkey)?;
                *current = Some(hotkey);
                info!("Registered hotkey: {}", hotkey_str);
            } else {
                error!("Failed to parse hotkey: {}", hotkey_str);
                return Err(anyhow::anyhow!("Invalid hotkey format"));
            }
        }
        
        Ok(())
    }
    
    pub fn start_listening(self: Arc<Self>) {
        let sender = self.event_sender.clone();
        
        std::thread::spawn(move || {
            info!("Hotkey listener thread started");
            let receiver = GlobalHotKeyEvent::receiver();
            
            loop {
                if let Ok(event) = receiver.recv() {
                    debug!("Hotkey event received: {:?}", event);
                    
                    // Check if this is our save hotkey
                    if let Some(current_hotkey) = self.current_hotkey.lock().unwrap().as_ref() {
                        if event.id == current_hotkey.id() {
                            info!("Save hotkey triggered");
                            let _ = sender.blocking_send(HotkeyEvent::SaveNow);
                        }
                    }
                }
            }
        });
    }
    
    fn parse_hotkey(hotkey_str: &str) -> Result<HotKey> {
        // Parse hotkey string like "Ctrl+Shift+S" or "Alt+F5"
        let parts: Vec<&str> = hotkey_str.split('+').collect();
        if parts.is_empty() {
            return Err(anyhow::anyhow!("Empty hotkey string"));
        }
        
        let mut modifiers = Modifiers::empty();
        let mut key_code = None;
        
        for part in parts {
            match part.to_lowercase().as_str() {
                "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
                "alt" => modifiers |= Modifiers::ALT,
                "shift" => modifiers |= Modifiers::SHIFT,
                "super" | "win" | "cmd" | "meta" => modifiers |= Modifiers::META,
                key => {
                    // Try to parse as key code
                    key_code = Some(Self::str_to_code(key)?);
                }
            }
        }
        
        let code = key_code.ok_or_else(|| anyhow::anyhow!("No key specified"))?;
        Ok(HotKey::new(Some(modifiers), code))
    }
    
    fn str_to_code(key: &str) -> Result<Code> {
        // Convert string to Code enum
        match key.to_lowercase().as_str() {
            "a" => Ok(Code::KeyA),
            "b" => Ok(Code::KeyB),
            "c" => Ok(Code::KeyC),
            "d" => Ok(Code::KeyD),
            "e" => Ok(Code::KeyE),
            "f" => Ok(Code::KeyF),
            "g" => Ok(Code::KeyG),
            "h" => Ok(Code::KeyH),
            "i" => Ok(Code::KeyI),
            "j" => Ok(Code::KeyJ),
            "k" => Ok(Code::KeyK),
            "l" => Ok(Code::KeyL),
            "m" => Ok(Code::KeyM),
            "n" => Ok(Code::KeyN),
            "o" => Ok(Code::KeyO),
            "p" => Ok(Code::KeyP),
            "q" => Ok(Code::KeyQ),
            "r" => Ok(Code::KeyR),
            "s" => Ok(Code::KeyS),
            "t" => Ok(Code::KeyT),
            "u" => Ok(Code::KeyU),
            "v" => Ok(Code::KeyV),
            "w" => Ok(Code::KeyW),
            "x" => Ok(Code::KeyX),
            "y" => Ok(Code::KeyY),
            "z" => Ok(Code::KeyZ),
            "0" => Ok(Code::Digit0),
            "1" => Ok(Code::Digit1),
            "2" => Ok(Code::Digit2),
            "3" => Ok(Code::Digit3),
            "4" => Ok(Code::Digit4),
            "5" => Ok(Code::Digit5),
            "6" => Ok(Code::Digit6),
            "7" => Ok(Code::Digit7),
            "8" => Ok(Code::Digit8),
            "9" => Ok(Code::Digit9),
            "f1" => Ok(Code::F1),
            "f2" => Ok(Code::F2),
            "f3" => Ok(Code::F3),
            "f4" => Ok(Code::F4),
            "f5" => Ok(Code::F5),
            "f6" => Ok(Code::F6),
            "f7" => Ok(Code::F7),
            "f8" => Ok(Code::F8),
            "f9" => Ok(Code::F9),
            "f10" => Ok(Code::F10),
            "f11" => Ok(Code::F11),
            "f12" => Ok(Code::F12),
            "space" => Ok(Code::Space),
            "enter" | "return" => Ok(Code::Enter),
            "tab" => Ok(Code::Tab),
            "escape" | "esc" => Ok(Code::Escape),
            "backspace" => Ok(Code::Backspace),
            "delete" | "del" => Ok(Code::Delete),
            "insert" | "ins" => Ok(Code::Insert),
            "home" => Ok(Code::Home),
            "end" => Ok(Code::End),
            "pageup" | "pgup" => Ok(Code::PageUp),
            "pagedown" | "pgdown" => Ok(Code::PageDown),
            "up" => Ok(Code::ArrowUp),
            "down" => Ok(Code::ArrowDown),
            "left" => Ok(Code::ArrowLeft),
            "right" => Ok(Code::ArrowRight),
            _ => Err(anyhow::anyhow!("Unknown key: {}", key))
        }
    }
    
    pub fn get_current_hotkey(&self) -> Option<String> {
        // Return current hotkey as string for display
        // This is a simplified version - you might want to store the original string
        if self.current_hotkey.lock().unwrap().is_some() {
            Some("Ctrl+Shift+S".to_string()) // Default for now
        } else {
            None
        }
    }
}