use std::io::Read;
use std::path::Path;
use chrono::{DateTime, Utc, TimeZone};
use tracing::debug;

/// GameCube save file (.gci) parser
pub struct GCIFile {
    pub game_code: String,
    pub maker_code: String,
    pub filename: String,
    pub modified_time: DateTime<Utc>,
    pub block_count: u16,
}

impl GCIFile {
    /// Parse a GCI file and extract metadata
    pub fn parse<P: AsRef<Path>>(path: P) -> Option<Self> {
        let mut file = std::fs::File::open(path.as_ref()).ok()?;
        let mut header = [0u8; 0x40];
        file.read_exact(&mut header).ok()?;
        
        // Extract game code (4 bytes)
        let game_code = String::from_utf8_lossy(&header[0..4])
            .trim_end_matches('\0')
            .to_string();
        
        // Extract maker code (2 bytes)
        let maker_code = String::from_utf8_lossy(&header[4..6])
            .trim_end_matches('\0')
            .to_string();
        
        // Extract filename (32 bytes, starting at offset 0x08)
        let filename = String::from_utf8_lossy(&header[0x08..0x28])
            .trim_end_matches('\0')
            .to_string();
        
        // Extract modification time (4 bytes at offset 0x28)
        // Time is seconds since 2000-01-01 00:00:00
        let mod_time_bytes = [header[0x28], header[0x29], header[0x2A], header[0x2B]];
        let mod_time_seconds = u32::from_be_bytes(mod_time_bytes);
        
        // Convert to DateTime
        let base_time = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
        let modified_time = base_time + chrono::Duration::seconds(mod_time_seconds as i64);
        
        // Extract block count (2 bytes at offset 0x38)
        let block_count_bytes = [header[0x38], header[0x39]];
        let block_count = u16::from_be_bytes(block_count_bytes);
        
        debug!(
            "Parsed GCI file: game_code={}, maker={}, filename={}, modified={}, blocks={}",
            game_code, maker_code, filename, modified_time, block_count
        );
        
        Some(GCIFile {
            game_code,
            maker_code,
            filename,
            modified_time,
            block_count,
        })
    }
    
    /// Get the full game ID (game code + maker code)
    /// Standard format: XXXXNN where XXXX is game code (includes region) and NN is maker code
    pub fn get_game_id(&self) -> String {
        // Game code already includes region as 4th character
        // Format: [Game ID (3 chars)][Region (1 char)][Maker (2 chars)]
        format!("{}{}", self.game_code, self.maker_code)
    }
    
    /// Get a human-readable save description
    pub fn get_save_description(&self) -> String {
        // Clean up the filename which might contain special characters
        let clean_filename = self.filename
            .chars()
            .filter(|c| c.is_ascii_graphic() || c.is_whitespace())
            .collect::<String>()
            .trim()
            .to_string();
        
        if clean_filename.is_empty() {
            format!("{} Save", self.game_code)
        } else {
            clean_filename
        }
    }
    
    /// Extract game ID from GCI filename pattern
    /// Pattern: {makercode}-{gamecode}-{filename}.gci
    /// Example: 01-GZLE-gczelda.gci
    pub fn extract_game_id_from_filename(filename: &str) -> Option<String> {
        let parts: Vec<&str> = filename.split('-').collect();
        if parts.len() >= 2 {
            // Second part should be the game code
            let game_code = parts[1];
            if game_code.len() == 4 {
                return Some(game_code.to_string());
            }
        }
        None
    }
}

/// Parse all GCI files in a directory and extract game information
pub fn scan_gci_directory<P: AsRef<Path>>(dir: P) -> Vec<(String, String)> {
    let mut games = Vec::new();
    
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "gci") {
                if let Some(gci) = GCIFile::parse(&path) {
                    games.push((gci.game_code.clone(), gci.get_save_description()));
                } else if let Some(filename) = path.file_name() {
                    // Try to extract from filename if parsing fails
                    let filename_str = filename.to_string_lossy();
                    if let Some(game_id) = GCIFile::extract_game_id_from_filename(&filename_str) {
                        games.push((game_id, filename_str.to_string()));
                    }
                }
            }
        }
    }
    
    games
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_extract_game_id_from_filename() {
        assert_eq!(
            GCIFile::extract_game_id_from_filename("01-GZLE-gczelda.gci"),
            Some("GZLE".to_string())
        );
        
        assert_eq!(
            GCIFile::extract_game_id_from_filename("8P-GALE-ssbtitle.gci"),
            Some("GALE".to_string())
        );
        
        assert_eq!(
            GCIFile::extract_game_id_from_filename("invalid.gci"),
            None
        );
    }
}