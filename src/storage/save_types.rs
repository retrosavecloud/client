use serde::{Deserialize, Serialize};
use std::path::Path;

/// Different types of save data formats used by emulators
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SaveType {
    /// Memory card file containing multiple game saves
    MemoryCard { 
        format: MemoryCardFormat,
        contains_saves: bool,
        save_count: u32,
    },
    /// Individual save file for a single game
    IndividualFile { 
        game_id: String,
    },
    /// Save state (snapshot of emulator state)
    SaveState { 
        slot: Option<u8>,
        game_id: String,
    },
    /// Folder structure containing save data
    SaveFolder { 
        structure: FolderStructure,
        game_id: String,
    },
}

/// Types of memory card formats
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MemoryCardFormat {
    PS2,      // 8MB fixed size, .ps2 extension
    PS1,      // 128KB fixed size, .mcr/.mcd extension  
    GameCube, // Variable size, .raw/.gcp extension
    Unknown,
}

/// Types of save folder structures
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FolderStructure {
    RPCS3,    // PS3: savedata/GAME_ID/
    Yuzu,     // Switch: save/user_id/title_id/
    Citra,    // 3DS: Nintendo 3DS/ID0/ID1/title/
    PPSSPP,   // PSP: SAVEDATA/GAME_ID_SAVE/
}

impl SaveType {
    /// Detect save type from file path and emulator
    pub fn detect(file_path: &Path, emulator: &str) -> Self {
        let extension = file_path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        
        match emulator.to_lowercase().as_str() {
            "pcsx2" => {
                if extension == "ps2" {
                    SaveType::MemoryCard {
                        format: MemoryCardFormat::PS2,
                        contains_saves: false, // Will be determined by content check
                        save_count: 0,
                    }
                } else {
                    SaveType::SaveState {
                        slot: None,
                        game_id: String::new(),
                    }
                }
            },
            "dolphin" => {
                if extension == "raw" || extension == "gcp" || extension == "gci" {
                    SaveType::MemoryCard {
                        format: MemoryCardFormat::GameCube,
                        contains_saves: false,
                        save_count: 0,
                    }
                } else {
                    SaveType::SaveState {
                        slot: None,
                        game_id: String::new(),
                    }
                }
            },
            "rpcs3" => {
                // RPCS3 uses folder structure
                SaveType::SaveFolder {
                    structure: FolderStructure::RPCS3,
                    game_id: file_path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string(),
                }
            },
            "retroarch" => {
                // RetroArch uses individual save files
                let game_name = file_path.file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                
                if extension == "srm" || extension == "sav" {
                    SaveType::IndividualFile {
                        game_id: game_name,
                    }
                } else {
                    SaveType::SaveState {
                        slot: None,
                        game_id: game_name,
                    }
                }
            },
            _ => {
                // Default to individual file
                SaveType::IndividualFile {
                    game_id: file_path.file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string(),
                }
            }
        }
    }
    
    /// Check if this save type uses memory cards
    pub fn is_memory_card(&self) -> bool {
        matches!(self, SaveType::MemoryCard { .. })
    }
    
    /// Check if this save type represents individual game saves
    pub fn is_individual(&self) -> bool {
        matches!(self, SaveType::IndividualFile { .. } | SaveType::SaveFolder { .. })
    }
}

/// Memory card content detection
impl MemoryCardFormat {
    /// Check if a memory card is empty (formatted but no saves)
    pub fn is_empty(&self, data: &[u8]) -> bool {
        match self {
            MemoryCardFormat::PS2 => {
                // PS2 memory cards are 8,650,752 bytes (8MB + filesystem)
                // Check if it's just formatted (has header but no saves)
                if data.len() < 0x2000 {
                    return true; // Too small to be valid
                }
                
                // Check for Sony Computer Entertainment header
                let has_header = data.len() >= 4 && &data[0..4] == b"Sony";
                if !has_header {
                    return true; // Not even formatted properly
                }
                
                // Check if directory entries exist (starting at 0x0200)
                // Empty cards have all 0xFF or 0x00 in the directory area
                if data.len() >= 0x2000 {
                    let directory_area = &data[0x0200..0x2000];
                    let has_saves = directory_area.iter()
                        .any(|&byte| byte != 0xFF && byte != 0x00);
                    !has_saves
                } else {
                    true
                }
            },
            MemoryCardFormat::PS1 => {
                // PS1 memory cards are 131,072 bytes (128KB)
                if data.len() != 131072 {
                    return true;
                }
                
                // Check for "MC" header
                if &data[0..2] != b"MC" {
                    return true;
                }
                
                // Check directory frames (starting at 0x0080)
                let directory = &data[0x0080..0x0800];
                !directory.iter().any(|&b| b != 0x00 && b != 0xFF)
            },
            MemoryCardFormat::GameCube => {
                // GameCube memory cards vary in size
                // Check for valid header
                if data.len() < 0x2000 {
                    return true;
                }
                
                // Basic check for now
                data.iter().all(|&b| b == 0xFF || b == 0x00)
            },
            MemoryCardFormat::Unknown => {
                // Can't determine, assume not empty
                false
            }
        }
    }
    
    /// Count the number of saves in a memory card
    pub fn count_saves(&self, data: &[u8]) -> u32 {
        match self {
            MemoryCardFormat::PS2 => {
                if data.len() < 0x2000 {
                    return 0;
                }
                
                // Count non-empty directory entries
                let mut count = 0;
                // Each directory entry is 512 bytes
                for i in 0..15 {
                    let offset = 0x0200 + (i * 512);
                    if offset + 512 <= data.len() {
                        let entry = &data[offset..offset + 512];
                        // Check if entry is not empty
                        if entry.iter().any(|&b| b != 0x00 && b != 0xFF) {
                            count += 1;
                        }
                    }
                }
                count
            },
            _ => 0, // TODO: Implement for other formats
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    
    #[test]
    fn test_save_type_detection() {
        // Test PS2 memory card detection
        let path = PathBuf::from("/home/user/.config/PCSX2/memcards/1.ps2");
        let save_type = SaveType::detect(&path, "PCSX2");
        assert!(matches!(save_type, SaveType::MemoryCard { format: MemoryCardFormat::PS2, .. }));
        
        // Test RetroArch save detection
        let path = PathBuf::from("/home/user/.config/retroarch/saves/game.srm");
        let save_type = SaveType::detect(&path, "retroarch");
        assert!(matches!(save_type, SaveType::IndividualFile { .. }));
        
        // Test RPCS3 folder detection
        let path = PathBuf::from("/home/user/.config/rpcs3/savedata/BLUS30443");
        let save_type = SaveType::detect(&path, "rpcs3");
        assert!(matches!(save_type, SaveType::SaveFolder { structure: FolderStructure::RPCS3, .. }));
    }
    
    #[test]
    fn test_empty_ps2_memory_card() {
        // Create a mock empty PS2 memory card
        let mut data = vec![0xFF; 8650752];
        data[0..4].copy_from_slice(b"Sony"); // Add header
        
        let format = MemoryCardFormat::PS2;
        assert!(format.is_empty(&data));
        assert_eq!(format.count_saves(&data), 0);
    }
}