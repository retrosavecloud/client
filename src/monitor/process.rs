use sysinfo::{System, ProcessesToUpdate};
use tracing::debug;
use std::process::Command;

#[derive(Debug, Clone)]
pub enum EmulatorProcess {
    PCSX2 {
        pid: u32,
        exe_path: String,
    },
    // Future emulators
    // Dolphin { pid: u32, exe_path: String },
    // RPCS3 { pid: u32, exe_path: String },
}

pub fn detect_running_emulators() -> Option<EmulatorProcess> {
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    
    for (pid, process) in system.processes() {
        let process_name = process.name().to_string_lossy().to_lowercase();
        
        // Check for PCSX2
        if process_name.contains("pcsx2") {
            debug!("Found PCSX2 process: {:?} (PID: {})", process.name(), pid);
            
            let exe_path = process
                .exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            
            return Some(EmulatorProcess::PCSX2 {
                pid: pid.as_u32(),
                exe_path,
            });
        }
        
        // Future: Add more emulator checks here
    }
    
    None
}

pub fn get_pcsx2_save_directory() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(home) = std::env::var("USERPROFILE") {
            let save_path = format!("{}\\Documents\\PCSX2\\memcards", home);
            if std::path::Path::new(&save_path).exists() {
                return Some(save_path);
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            // Check Flatpak location first (most common nowadays)
            let flatpak_path = format!("{}/.var/app/net.pcsx2.PCSX2/config/PCSX2/memcards", home);
            if std::path::Path::new(&flatpak_path).exists() {
                return Some(flatpak_path);
            }
            
            // Check new location
            let save_path = format!("{}/.config/PCSX2/memcards", home);
            if std::path::Path::new(&save_path).exists() {
                return Some(save_path);
            }
            
            // Check old location
            let old_save_path = format!("{}/.pcsx2/memcards", home);
            if std::path::Path::new(&old_save_path).exists() {
                return Some(old_save_path);
            }
        }
    }
    
    None
}

/// Try to get the current game name from PCSX2
pub fn get_pcsx2_game_name(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        // Try to get window title using xdotool
        let output = Command::new("xdotool")
            .args(&["search", "--pid", &pid.to_string(), "getwindowname"])
            .output()
            .ok()?;
        
        if output.status.success() {
            let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
            debug!("PCSX2 window title: {}", title);
            
            // PCSX2 window title formats:
            // 1. "Game Title | PCSX2"
            // 2. "PCSX2 - Game Title [Status]"
            // 3. "Game Title - PCSX2 1.7.x"
            // 4. Just "PCSX2" when no game is running
            
            if title == "PCSX2" || title.is_empty() {
                return None;
            }
            
            if title.contains(" | PCSX2") {
                let parts: Vec<&str> = title.split(" | PCSX2").collect();
                if !parts.is_empty() && !parts[0].is_empty() {
                    return Some(parts[0].to_string());
                }
            } else if title.contains(" - PCSX2") {
                let parts: Vec<&str> = title.split(" - PCSX2").collect();
                if !parts.is_empty() && !parts[0].is_empty() {
                    return Some(parts[0].to_string());
                }
            } else if title.starts_with("PCSX2") && title.contains(" - ") {
                let parts: Vec<&str> = title.split(" - ").collect();
                if parts.len() > 1 {
                    // Remove any [Status] tags
                    let game = parts[1].split('[').next().unwrap_or(parts[1]).trim();
                    if !game.is_empty() && game != "No Game Running" {
                        return Some(game.to_string());
                    }
                }
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        // Windows implementation would use Win32 API
        // For now, return None
    }
    
    None
}