use sysinfo::{System, ProcessesToUpdate};
use tracing::debug;

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