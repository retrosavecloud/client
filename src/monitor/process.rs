use sysinfo::{System, ProcessesToUpdate};
use tracing::{debug, info};
use std::path::{Path, PathBuf};
use std::fs;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub enum EmulatorProcess {
    PCSX2 {
        pid: u32,
        exe_path: String,
    },
    Dolphin {
        pid: u32,
        exe_path: String,
    },
    // Future emulators
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
        
        // Check for Dolphin
        if process_name.contains("dolphin") {
            debug!("Found Dolphin process: {:?} (PID: {})", process.name(), pid);
            
            let exe_path = process
                .exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            
            return Some(EmulatorProcess::Dolphin {
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

/// Try to get the current game name from PCSX2 using multiple methods
pub fn get_pcsx2_game_name(pid: u32) -> Option<String> {
    info!("Attempting to detect PCSX2 game for PID {}", pid);
    
    // Method 1: Try to get from window title (most accurate for running game)
    if let Some(game_name) = get_game_from_window_title(pid) {
        info!("Got game from window title: {}", game_name);
        return Some(game_name);
    }
    
    // Method 2: Check process command line arguments
    if let Some(game_info) = get_game_from_process_cmd(pid) {
        info!("Got game from command line: {}", game_info);
        return Some(game_info);
    }
    
    // Method 3: Check game settings files (least reliable - might be old game)
    if let Some(game_info) = get_game_from_settings_files() {
        info!("Got game from settings file: {}", game_info);
        return Some(game_info);
    }
    
    info!("Could not detect game name for PID {}", pid);
    None
}

/// Get game name from PCSX2 window title using native platform APIs
fn get_game_from_window_title(_pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        get_game_from_window_title_linux()
    }
    
    #[cfg(target_os = "windows")]
    {
        get_game_from_window_title_windows()
    }
    
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn get_game_from_window_title_linux() -> Option<String> {
    use x11::xlib;
    
    debug!("Starting X11 window title detection for PCSX2");
    
    unsafe {
        // Open X11 display
        let display = xlib::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            debug!("Failed to open X11 display");
            return None;
        }
        
        // Get root window
        let root = xlib::XDefaultRootWindow(display);
        
        // Get window list property
        let mut actual_type = 0;
        let mut actual_format = 0;
        let mut num_items = 0;
        let mut bytes_after = 0;
        let mut properties: *mut u8 = std::ptr::null_mut();
        
        let net_client_list = xlib::XInternAtom(
            display,
            b"_NET_CLIENT_LIST\0".as_ptr() as *const i8,
            xlib::False
        );
        
        if xlib::XGetWindowProperty(
            display,
            root,
            net_client_list,
            0,
            1024,
            xlib::False,
            xlib::XA_WINDOW,
            &mut actual_type,
            &mut actual_format,
            &mut num_items,
            &mut bytes_after,
            &mut properties
        ) == 0 && !properties.is_null() {
            let windows = std::slice::from_raw_parts(
                properties as *const xlib::Window,
                num_items as usize
            );
            
            debug!("Found {} windows to check", num_items);
            
            let net_wm_name = xlib::XInternAtom(
                display,
                b"_NET_WM_NAME\0".as_ptr() as *const i8,
                xlib::False
            );
            let utf8_string = xlib::XInternAtom(
                display,
                b"UTF8_STRING\0".as_ptr() as *const i8,
                xlib::False
            );
            
            for &window in windows {
                // Get window class to check if it's PCSX2
                let mut class_hint = xlib::XClassHint {
                    res_name: std::ptr::null_mut(),
                    res_class: std::ptr::null_mut(),
                };
                
                if xlib::XGetClassHint(display, window, &mut class_hint) != 0 {
                    let is_pcsx2 = if !class_hint.res_name.is_null() {
                        let class_name = std::ffi::CStr::from_ptr(class_hint.res_name)
                            .to_string_lossy()
                            .to_lowercase();
                        xlib::XFree(class_hint.res_name as *mut _);
                        if !class_hint.res_class.is_null() {
                            xlib::XFree(class_hint.res_class as *mut _);
                        }
                        class_name.contains("pcsx2")
                    } else {
                        if !class_hint.res_class.is_null() {
                            xlib::XFree(class_hint.res_class as *mut _);
                        }
                        false
                    };
                    
                    if is_pcsx2 {
                        // Get window title
                        let mut title_type = 0;
                        let mut title_format = 0;
                        let mut title_items = 0;
                        let mut title_bytes = 0;
                        let mut title_prop: *mut u8 = std::ptr::null_mut();
                        
                        if xlib::XGetWindowProperty(
                            display,
                            window,
                            net_wm_name,
                            0,
                            1024,
                            xlib::False,
                            utf8_string,
                            &mut title_type,
                            &mut title_format,
                            &mut title_items,
                            &mut title_bytes,
                            &mut title_prop
                        ) == 0 && !title_prop.is_null() {
                            let title = std::ffi::CStr::from_ptr(title_prop as *const i8)
                                .to_string_lossy()
                                .to_string();
                            xlib::XFree(title_prop as *mut _);
                            
                            debug!("Found PCSX2 window with title: {}", title);
                            
                            // Skip generic PCSX2 titles and empty titles
                            if !title.is_empty() && !title.starts_with("PCSX2") && title != "pcsx2-qt" {
                                xlib::XFree(properties as *mut _);
                                xlib::XCloseDisplay(display);
                                info!("Detected game from X11 window title: {}", title);
                                return Some(title);
                            }
                        } else {
                            // Try fallback to WM_NAME if _NET_WM_NAME doesn't work
                            let mut title_prop: *mut i8 = std::ptr::null_mut();
                            if xlib::XFetchName(display, window, &mut title_prop) != 0 && !title_prop.is_null() {
                                let title = std::ffi::CStr::from_ptr(title_prop)
                                    .to_string_lossy()
                                    .to_string();
                                xlib::XFree(title_prop as *mut _);
                                
                                debug!("Found PCSX2 window with WM_NAME: {}", title);
                                
                                if !title.is_empty() && !title.starts_with("PCSX2") && title != "pcsx2-qt" {
                                    xlib::XFree(properties as *mut _);
                                    xlib::XCloseDisplay(display);
                                    info!("Detected game from X11 WM_NAME: {}", title);
                                    return Some(title);
                                }
                            }
                        }
                    }
                }
            }
            
            xlib::XFree(properties as *mut _);
        }
        
        xlib::XCloseDisplay(display);
    }
    
    None
}

#[cfg(target_os = "windows")]
fn get_game_from_window_title_windows() -> Option<String> {
    use winapi::um::winuser::{EnumWindows, GetWindowTextW, GetClassNameW};
    use winapi::shared::minwindef::{LPARAM, BOOL, TRUE, FALSE};
    use winapi::shared::windef::HWND;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    
    struct EnumData {
        game_title: Option<String>,
    }
    
    unsafe extern "system" fn enum_window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam as *mut EnumData);
        
        // Get window class name
        let mut class_name = [0u16; 256];
        let class_len = GetClassNameW(hwnd, class_name.as_mut_ptr(), 256);
        
        if class_len > 0 {
            let class = OsString::from_wide(&class_name[..class_len as usize])
                .to_string_lossy()
                .to_lowercase();
            
            // Check if it's a PCSX2 window
            if class.contains("pcsx2") || class.contains("qt") {
                // Get window title
                let mut title = [0u16; 512];
                let title_len = GetWindowTextW(hwnd, title.as_mut_ptr(), 512);
                
                if title_len > 0 {
                    let title_str = OsString::from_wide(&title[..title_len as usize])
                        .to_string_lossy()
                        .to_string();
                    
                    // Skip generic PCSX2 titles
                    if !title_str.starts_with("PCSX2") && title_str != "pcsx2-qt" {
                        data.game_title = Some(title_str);
                        return FALSE; // Stop enumeration
                    }
                }
            }
        }
        
        TRUE // Continue enumeration
    }
    
    let mut data = EnumData { game_title: None };
    
    unsafe {
        EnumWindows(Some(enum_window_proc), &mut data as *mut _ as LPARAM);
    }
    
    data.game_title
}

/// Get game info from PCSX2 game settings files
fn get_game_from_settings_files() -> Option<String> {
    let settings_dirs = vec![
        // Flatpak location
        format!("{}/.var/app/net.pcsx2.PCSX2/config/PCSX2/gamesettings", std::env::var("HOME").ok()?),
        // Standard location
        format!("{}/.config/PCSX2/gamesettings", std::env::var("HOME").ok()?),
        // Old location
        format!("{}/.pcsx2/gamesettings", std::env::var("HOME").ok()?),
    ];
    
    for dir_path in settings_dirs {
        let path = Path::new(&dir_path);
        if !path.exists() {
            continue;
        }
        
        // Find the most recently modified .ini file
        let mut most_recent: Option<(PathBuf, SystemTime)> = None;
        
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("ini") {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            if most_recent.is_none() || modified > most_recent.as_ref().unwrap().1 {
                                most_recent = Some((path, modified));
                            }
                        }
                    }
                }
            }
        }
        
        if let Some((path, _)) = most_recent {
            // Extract game ID from filename (e.g., "SLUS-20552_248E6126.ini")
            if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
                if let Some(game_id) = filename.split('_').next() {
                    // Try to get a friendly name for known games
                    let friendly_name = get_friendly_game_name(game_id);
                    return Some(friendly_name.unwrap_or_else(|| {
                        // If we don't know the game, at least show the ID
                        format!("PS2 Game [{}]", game_id)
                    }));
                }
            }
        }
    }
    
    None
}

/// Get game info from process command line arguments
fn get_game_from_process_cmd(pid: u32) -> Option<String> {
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    
    for (process_pid, process) in system.processes() {
        if process_pid.as_u32() == pid {
            // Get command line arguments
            let cmd = process.cmd();
            debug!("PCSX2 command line: {:?}", cmd);
            
            // Look for ISO or ELF file in arguments
            for arg in cmd {
                let arg_str = arg.to_string_lossy();
                if arg_str.ends_with(".iso") || arg_str.ends_with(".ISO") ||
                   arg_str.ends_with(".elf") || arg_str.ends_with(".ELF") ||
                   arg_str.ends_with(".bin") || arg_str.ends_with(".BIN") {
                    // Extract filename without extension
                    if let Some(path) = Path::new(&*arg_str).file_stem() {
                        let game_name = path.to_string_lossy().to_string();
                        // Clean up the name (remove underscores, etc.)
                        let clean_name = game_name.replace('_', " ");
                        return Some(clean_name);
                    }
                }
            }
            break;
        }
    }
    
    None
}

/// Map game IDs to friendly names for known games
fn get_friendly_game_name(game_id: &str) -> Option<String> {
    // This is a small subset - can be expanded over time
    // Format: Game ID -> Game Name (CORRECTED)
    match game_id {
        // US Games (SLUS)
        "SLUS-20328" => Some("Tekken 4".to_string()),
        "SLUS-20552" => Some("Grand Theft Auto: Vice City".to_string()),
        "SLUS-20826" => Some("Harry Potter and the Sorcerer's Stone".to_string()),
        "SLUS-21065" => Some("Tales of the Abyss".to_string()),
        "SLUS-20001" => Some("Final Fantasy X".to_string()),
        "SLUS-20488" => Some("Final Fantasy X-2".to_string()),
        "SLUS-20672" => Some("Final Fantasy XII".to_string()),
        "SLUS-20035" => Some("Metal Gear Solid 2".to_string()),
        "SLUS-20789" => Some("Metal Gear Solid 3".to_string()),
        "SLUS-20062" => Some("Grand Theft Auto III".to_string()),
        "SLUS-20946" => Some("Grand Theft Auto: San Andreas".to_string()),
        "SLUS-20216" => Some("Kingdom Hearts".to_string()),
        "SLUS-21005" => Some("Kingdom Hearts II".to_string()),
        
        // European Games (SLES)
        "SLES-50326" => Some("Final Fantasy X".to_string()),
        "SLES-52409" => Some("Final Fantasy X-2".to_string()),
        "SLES-50490" => Some("Final Fantasy XII".to_string()),
        
        // Japanese Games (SLPS/SLPM)
        "SLPS-25088" => Some("Final Fantasy X International".to_string()),
        "SLPM-65051" => Some("Metal Gear Solid 2".to_string()),
        
        // Add more mappings as needed
        _ => None
    }
}

/// Try to get the current game name from Dolphin
pub fn get_dolphin_game_name(pid: u32) -> Option<String> {
    info!("Attempting to detect Dolphin game for PID {}", pid);
    
    // Method 1: Try to get from window title
    if let Some(game_name) = get_dolphin_game_from_window_title(pid) {
        info!("Got Dolphin game from window title: {}", game_name);
        return Some(game_name);
    }
    
    // Method 2: Check process command line arguments
    if let Some(game_info) = get_dolphin_game_from_process_cmd(pid) {
        info!("Got Dolphin game from command line: {}", game_info);
        return Some(game_info);
    }
    
    // Method 3: Check recent game from config
    if let Some(game_info) = get_dolphin_game_from_config() {
        info!("Got Dolphin game from config: {}", game_info);
        return Some(game_info);
    }
    
    None
}

fn get_dolphin_game_from_window_title(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            // Similar to PCSX2 but looking for Dolphin windows
            let display = x11::xlib::XOpenDisplay(std::ptr::null());
            if display.is_null() {
                return None;
            }
            
            let root = x11::xlib::XDefaultRootWindow(display);
            let mut root_return = 0;
            let mut parent_return = 0;
            let mut children: *mut x11::xlib::Window = std::ptr::null_mut();
            let mut n_children = 0;
            
            if x11::xlib::XQueryTree(
                display,
                root,
                &mut root_return,
                &mut parent_return,
                &mut children,
                &mut n_children
            ) == 0 {
                x11::xlib::XCloseDisplay(display);
                return None;
            }
            
            let windows = std::slice::from_raw_parts(children, n_children as usize);
            let net_wm_name = x11::xlib::XInternAtom(
                display,
                b"_NET_WM_NAME\0".as_ptr() as *const i8,
                x11::xlib::False
            );
            let utf8_string = x11::xlib::XInternAtom(
                display,
                b"UTF8_STRING\0".as_ptr() as *const i8,
                x11::xlib::False
            );
            
            for &window in windows {
                // Get window class to check if it's Dolphin
                let mut class_hint = x11::xlib::XClassHint {
                    res_name: std::ptr::null_mut(),
                    res_class: std::ptr::null_mut(),
                };
                
                if x11::xlib::XGetClassHint(display, window, &mut class_hint) != 0 {
                    let is_dolphin = if !class_hint.res_name.is_null() {
                        let class_name = std::ffi::CStr::from_ptr(class_hint.res_name)
                            .to_string_lossy()
                            .to_lowercase();
                        x11::xlib::XFree(class_hint.res_name as *mut _);
                        if !class_hint.res_class.is_null() {
                            x11::xlib::XFree(class_hint.res_class as *mut _);
                        }
                        class_name.contains("dolphin")
                    } else {
                        if !class_hint.res_class.is_null() {
                            x11::xlib::XFree(class_hint.res_class as *mut _);
                        }
                        false
                    };
                    
                    if is_dolphin {
                        // Get window title
                        let mut title_type = 0;
                        let mut title_format = 0;
                        let mut title_items = 0;
                        let mut title_bytes = 0;
                        let mut title_prop: *mut u8 = std::ptr::null_mut();
                        
                        if x11::xlib::XGetWindowProperty(
                            display,
                            window,
                            net_wm_name,
                            0,
                            1024,
                            x11::xlib::False,
                            utf8_string,
                            &mut title_type,
                            &mut title_format,
                            &mut title_items,
                            &mut title_bytes,
                            &mut title_prop
                        ) == 0 && !title_prop.is_null() {
                            let title = std::ffi::CStr::from_ptr(title_prop as *const i8)
                                .to_string_lossy()
                                .to_string();
                            x11::xlib::XFree(title_prop as *mut _);
                            
                            // Dolphin window title format: "Game Title | Dolphin"
                            if let Some(game_part) = title.split(" | ").next() {
                                if !game_part.is_empty() && game_part != "Dolphin" {
                                    x11::xlib::XFree(children as *mut _);
                                    x11::xlib::XCloseDisplay(display);
                                    return Some(game_part.to_string());
                                }
                            }
                        }
                    }
                }
            }
            
            x11::xlib::XFree(children as *mut _);
            x11::xlib::XCloseDisplay(display);
        }
    }
    
    None
}

fn get_dolphin_game_from_process_cmd(pid: u32) -> Option<String> {
    // Check if Dolphin was launched with a game file
    let proc_cmdline = format!("/proc/{}/cmdline", pid);
    if let Ok(cmdline) = fs::read_to_string(&proc_cmdline) {
        let args: Vec<&str> = cmdline.split('\0').collect();
        for arg in args {
            // Check for common game file extensions
            if arg.ends_with(".iso") || arg.ends_with(".gcm") || arg.ends_with(".wbfs") || 
               arg.ends_with(".ciso") || arg.ends_with(".gcz") || arg.ends_with(".rvz") {
                if let Some(filename) = Path::new(arg).file_stem() {
                    return Some(filename.to_string_lossy().to_string());
                }
            }
        }
    }
    None
}

fn get_dolphin_game_from_config() -> Option<String> {
    // Try to read the last played game from Dolphin's config
    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            // Check various Dolphin config locations
            let config_paths = [
                format!("{}/.var/app/org.DolphinEmu.dolphin-emu/config/dolphin-emu/Dolphin.ini", home),
                format!("{}/.config/dolphin-emu/Dolphin.ini", home),
                format!("{}/.dolphin-emu/Config/Dolphin.ini", home),
            ];
            
            for config_path in &config_paths {
                if let Ok(content) = fs::read_to_string(config_path) {
                    // Look for LastFilename in the config
                    for line in content.lines() {
                        if line.starts_with("LastFilename = ") {
                            let path = line.trim_start_matches("LastFilename = ");
                            if let Some(filename) = Path::new(path).file_stem() {
                                return Some(filename.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        if let Ok(home) = std::env::var("USERPROFILE") {
            let config_path = format!("{}\\Documents\\Dolphin Emulator\\Config\\Dolphin.ini", home);
            if let Ok(content) = fs::read_to_string(&config_path) {
                for line in content.lines() {
                    if line.starts_with("LastFilename = ") {
                        let path = line.trim_start_matches("LastFilename = ");
                        if let Some(filename) = Path::new(path).file_stem() {
                            return Some(filename.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    
    None
}