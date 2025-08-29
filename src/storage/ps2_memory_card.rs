use std::collections::HashMap;
use crate::storage::game_database::lookup_game_name;
use retrosave_shared::{MemoryCardMetadata, GameInfo};
use chrono::{NaiveDate, NaiveTime, NaiveDateTime, Utc};
use tracing::debug;

/// PS2 Memory Card Parser
/// 
/// For detailed PS2 memory card structure documentation, see:
/// `/home/eralp/Projects/retrosave/docs/pcsx2/PS2_MEMORY_CARD_STRUCTURE.md`
/// 
/// Based on PCSX2 source code analysis:
/// - Standard size: 8,650,752 bytes (8MB card with ECC)
/// - Or 8,388,608 bytes (8MB without ECC)  
/// - Or 8,650,806 bytes (variant with extra ECC data)
/// - Format header: "Sony PS2 Memory Card Format" at offset 0
pub struct PS2MemoryCard {
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PS2Save {
    pub name: String,
    pub game_id: String,
    pub size: u32,
    pub modified: u32,  // Timestamp
    pub exists: bool,
}

impl PS2MemoryCard {
    pub fn new(data: Vec<u8>) -> Option<Self> {
        // Valid PS2 memory card sizes:
        // - 8,388,608 bytes (8MB raw without ECC)
        // - 8,650,752 bytes (8MB with ECC - most common)
        // - Some variants up to ~8.7MB
        if data.len() < 8_388_608 || data.len() > 8_700_000 {
            return None;
        }
        
        // Verify PS2 format signature
        const SONY_SIGNATURE: &[u8] = b"Sony PS2 Memory Card Format";
        if data.len() >= SONY_SIGNATURE.len() {
            if !data.starts_with(b"Sony") {
                return None;
            }
        }
        
        Some(PS2MemoryCard { data })
    }
    
    /// Parse saves by reading the proper directory structure
    pub fn parse_saves(&self) -> HashMap<String, PS2Save> {
        let mut saves = HashMap::new();
        
        // Read superblock to get root directory cluster (at offset 0x3C)
        if self.data.len() < 0x40 {
            return saves;
        }
        
        let rootdir_cluster = u32::from_le_bytes([
            self.data[0x3C],
            self.data[0x3D],
            self.data[0x3E],
            self.data[0x3F],
        ]);
        
        // Calculate directory offset (cluster * 1024)
        // Typically rootdir_cluster is 0x10, making dir_offset 0x4000
        // But some cards use 0x02, making it 0x2000
        let dir_offset = (rootdir_cluster as usize) * 1024;
        
        // Fallback to common offsets if calculated one seems wrong
        let offsets_to_try = if dir_offset == 0x4000 || dir_offset == 0x2000 {
            vec![dir_offset]
        } else {
            vec![0x2000, 0x4000, dir_offset]
        };
        
        for offset in offsets_to_try {
            if offset + 512 > self.data.len() {
                continue;
            }
            
            // Try to parse directory entries at this offset
            let parsed_saves = self.parse_directory_at_offset(offset);
            if !parsed_saves.is_empty() {
                return parsed_saves;
            }
        }
        
        // If directory parsing fails, fall back to scanning for game IDs
        self.fallback_scan_saves()
    }
    
    fn parse_directory_at_offset(&self, dir_offset: usize) -> HashMap<String, PS2Save> {
        let mut saves = HashMap::new();
        
        // PS2 cards can have up to 15 saves typically, but check more to be safe
        for i in 0..30 {  // Increased from 15 to be more thorough
            let entry_offset = dir_offset + (i * 512);
            if entry_offset + 512 > self.data.len() {
                break;
            }
            
            // Read mode flags (offset 0x00, 4 bytes)
            let mode = u32::from_le_bytes([
                self.data[entry_offset],
                self.data[entry_offset + 1],
                self.data[entry_offset + 2],
                self.data[entry_offset + 3],
            ]);
            
            // Skip if not used (0x8000) or not a file (0x0010)
            if mode & 0x8000 == 0 {
                continue; // Not in use
            }
            
            if mode & 0x0020 != 0 {
                continue; // It's a directory (0x0020 flag)
            }
            
            if mode & 0x0010 == 0 {
                continue; // Not a file
            }
            
            // Additional validation: mode shouldn't be 0xFFFFFFFF (unformatted)
            if mode == 0xFFFFFFFF {
                continue;
            }
            
            // Read size (offset 0x04, 4 bytes)
            let size = u32::from_le_bytes([
                self.data[entry_offset + 0x04],
                self.data[entry_offset + 0x05],
                self.data[entry_offset + 0x06],
                self.data[entry_offset + 0x07],
            ]);
            
            // Read timeModified (offset 0x18, 8 bytes)
            let time_modified = self.parse_ps2_datetime(entry_offset + 0x18);
            
            // Read name (offset 0x40, 32 bytes)
            let name_start = entry_offset + 0x40;
            let name_end = name_start + 32;
            if name_end > self.data.len() {
                continue;
            }
            
            let name_bytes = &self.data[name_start..name_end];
            let name = String::from_utf8_lossy(name_bytes)
                .trim_end_matches('\0')
                .to_string();
            
            if name.is_empty() {
                continue;
            }
            
            // Extract game ID from name
            let game_id = self.extract_game_id(&name);
            
            saves.insert(name.clone(), PS2Save {
                name: name.clone(),
                game_id,
                size,
                modified: time_modified,
                exists: true,
            });
        }
        
        saves
    }
    
    fn parse_ps2_datetime(&self, offset: usize) -> u32 {
        if offset + 8 > self.data.len() {
            return 0;
        }
        
        // PS2 datetime format (8 bytes):
        // [0]: unused (usually 0)
        // [1]: seconds (0-59)
        // [2]: minutes (0-59)
        // [3]: hours (0-23)
        // [4]: day (1-31)
        // [5]: month (1-12)
        // [6-7]: year (u16 little-endian)
        
        let _unused = self.data[offset];
        let second = self.data[offset + 1].min(59) as u32;
        let minute = self.data[offset + 2].min(59) as u32;
        let hour = self.data[offset + 3].min(23) as u32;
        let day = self.data[offset + 4] as u32;
        let month = self.data[offset + 5] as u32;
        let year = u16::from_le_bytes([
            self.data[offset + 6],
            self.data[offset + 7],
        ]) as u32;
        
        // Validate ranges
        if year < 1970 || year > 2100 || month == 0 || month > 12 || day == 0 || day > 31 {
            return 0;
        }
        
        // Convert to Unix timestamp
        if let Some(date) = NaiveDate::from_ymd_opt(year as i32, month, day) {
            if let Some(time) = NaiveTime::from_hms_opt(hour, minute, second) {
                let datetime = NaiveDateTime::new(date, time);
                return datetime.and_utc().timestamp() as u32;
            }
        }
        
        0
    }
    
    fn extract_game_id(&self, name: &str) -> String {
        // Save names typically start with B prefix + game serial
        // BESLES-52563FIFA05 -> extract SLES-52563
        // The game creates the save with a 'B' prefix (B+SLES becomes BESLES)
        
        debug!("Extracting game_id from save name: {}", name);
        
        // Remove the B prefix if present to get actual game serial
        let clean_name = if name.starts_with("BE") || name.starts_with("BA") {
            &name[1..]
        } else {
            name
        };
        
        // Common prefixes (after potentially removing B)
        let prefixes = vec![
            "ESLUS-", "ESLES-", "ASLES-", "ASLUS-",  // After removing B
            "SLUS-", "SLES-", "SCES-", "SCUS-",      // Direct prefixes
            "SLPM-", "SLPS-", "SCPS-", "SLKA-"
        ];
        
        for prefix in prefixes {
            if let Some(pos) = clean_name.find(prefix) {
                let start = pos;
                let mut end = start + prefix.len();
                
                // Read the numeric part (usually 5 digits)
                while end < clean_name.len() && end < start + prefix.len() + 5 {
                    if !clean_name.chars().nth(end).unwrap_or(' ').is_ascii_digit() {
                        break;
                    }
                    end += 1;
                }
                
                if end > start + prefix.len() {
                    let game_id = clean_name[start..end].to_string();
                    debug!("Extracted game_id: {}", game_id);
                    return game_id;
                }
            }
        }
        
        // If no standard game ID found, return the whole name
        debug!("Could not extract game_id, using full name: {}", name);
        name.to_string()
    }
    
    /// Fallback method: scan for game IDs in raw data
    fn fallback_scan_saves(&self) -> HashMap<String, PS2Save> {
        let mut saves = HashMap::new();
        let mut found_ids = std::collections::HashSet::new();
        
        let prefixes = vec![
            "BESLES-", "BASLUS-", "BASLES-", "BESLUS-",
            "SLES-", "SLUS-", "SCES-", "SCUS-",
            "SLPM-", "SLPS-", "SCPS-"
        ];
        
        for i in 0..self.data.len().saturating_sub(20) {
            for prefix in &prefixes {
                if i + prefix.len() + 10 > self.data.len() {
                    continue;
                }
                
                let prefix_bytes = prefix.as_bytes();
                if &self.data[i..i + prefix_bytes.len()] == prefix_bytes {
                    let mut end = i + prefix_bytes.len();
                    
                    // Read the numeric part
                    let mut valid_id = true;
                    for j in 0..5 {
                        if end + j >= self.data.len() {
                            valid_id = false;
                            break;
                        }
                        let ch = self.data[end + j];
                        if !ch.is_ascii_digit() {
                            valid_id = false;
                            break;
                        }
                    }
                    
                    if !valid_id {
                        continue;
                    }
                    
                    end += 5;
                    
                    // Look for optional suffix
                    let mut full_id_end = end;
                    if end < self.data.len() && self.data[end] == b'-' {
                        full_id_end = end + 1;
                        while full_id_end < self.data.len().min(end + 10) {
                            let ch = self.data[full_id_end];
                            if ch.is_ascii_alphanumeric() || ch == b'_' {
                                full_id_end += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    
                    if let Ok(full_id) = std::str::from_utf8(&self.data[i..full_id_end]) {
                        if !found_ids.contains(full_id) {
                            found_ids.insert(full_id.to_string());
                            
                            let base_id = if let Some(dash_pos) = full_id.rfind('-') {
                                if dash_pos > 8 {
                                    &full_id[..dash_pos]
                                } else {
                                    full_id
                                }
                            } else {
                                full_id
                            };
                            
                            saves.insert(full_id.to_string(), PS2Save {
                                name: full_id.to_string(),
                                game_id: base_id.to_string(),
                                size: 0,
                                modified: 0,
                                exists: true,
                            });
                        }
                    }
                }
            }
        }
        
        saves
    }
    
    /// Check if a specific game has saves
    pub fn has_game_saves(&self, game_name: &str) -> bool {
        let saves = self.parse_saves();
        
        saves.values().any(|save| {
            save.name.to_lowercase().contains(&game_name.to_lowercase()) ||
            save.game_id.to_lowercase().contains(&game_name.to_lowercase())
        })
    }
    
    /// Check if memory card has any saves at all
    pub fn has_any_saves(&self) -> bool {
        !self.parse_saves().is_empty()
    }
    
    /// Get a more detailed check for Harry Potter saves specifically
    pub fn has_harry_potter_save(&self) -> bool {
        let saves = self.parse_saves();
        
        saves.values().any(|save| {
            let name_lower = save.name.to_lowercase();
            let id_lower = save.game_id.to_lowercase();
            
            name_lower.contains("harry") || 
            name_lower.contains("potter") ||
            name_lower.contains("hp") ||
            id_lower.contains("52056") ||  // Harry Potter PAL
            id_lower.contains("52055") ||  // Other PAL version
            id_lower.contains("20826") ||  // NTSC-U
            id_lower.contains("65465") ||  // NTSC-J
            id_lower.contains("65650")     // NTSC-J EA Best Hits
        })
    }
    
    /// Get the most recently modified save
    pub fn get_last_modified_save(&self) -> Option<PS2Save> {
        let saves = self.parse_saves();
        saves.into_values()
            .filter(|s| s.modified > 0)
            .max_by_key(|s| s.modified)
    }
    
    /// Detect which save was modified between two memory card states
    pub fn detect_modified_save(&self, previous: &PS2MemoryCard) -> Option<String> {
        let current_saves = self.parse_saves();
        let previous_saves = previous.parse_saves();
        
        for (name, current_save) in current_saves.iter() {
            if let Some(prev_save) = previous_saves.get(name) {
                // Check if modified timestamp changed
                if current_save.modified > prev_save.modified {
                    return Some(name.clone());
                }
                // Also check size change as backup
                if current_save.size != prev_save.size {
                    return Some(name.clone());
                }
            } else {
                // New save that didn't exist before
                return Some(name.clone());
            }
        }
        
        None
    }
    
    /// Generate metadata about all games in this memory card
    pub fn generate_metadata(&self, primary_game: String) -> MemoryCardMetadata {
        let saves = self.parse_saves();
        
        let mut games_map: HashMap<String, Vec<String>> = HashMap::new();
        
        for (save_id, save) in saves.iter() {
            games_map.entry(save.game_id.clone())
                .or_insert_with(Vec::new)
                .push(save_id.clone());
        }
        
        let mut games_contained = Vec::new();
        for (game_id, save_ids) in games_map {
            let game_name = lookup_game_name(&game_id)
                .unwrap_or_else(|| game_id.clone());
            
            games_contained.push(GameInfo {
                game_id,
                game_name,
                save_count: save_ids.len(),
            });
        }
        
        games_contained.sort_by(|a, b| a.game_name.cmp(&b.game_name));
        
        MemoryCardMetadata {
            games_contained,
            primary_game,
            total_saves: saves.len(),
            format_version: "PS2_8MB".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_valid_memory_card() {
        let mut data = vec![0xFF; 8650752];
        data[0..4].copy_from_slice(b"Sony");
        
        let card = PS2MemoryCard::new(data);
        assert!(card.is_some());
    }
    
    #[test]
    fn test_invalid_memory_card() {
        let data = vec![0xFF; 100]; // Too small
        let card = PS2MemoryCard::new(data);
        assert!(card.is_none());
    }
}