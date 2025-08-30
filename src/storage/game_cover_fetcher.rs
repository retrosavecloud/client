use anyhow::Result;
use tracing::{debug, info, warn};

/// Fetches game cover art using game IDs
pub struct GameCoverFetcher;

impl GameCoverFetcher {
    /// Get cover art URL for a PS2 game ID
    pub fn get_ps2_cover_url(game_id: &str) -> Option<String> {
        // Clean up the ID (remove BE/BA prefix and any suffix)
        let clean_id = Self::normalize_game_id(game_id);
        
        // Determine region from ID
        let region = if clean_id.starts_with("SLUS") || clean_id.starts_with("SCUS") {
            "US"
        } else if clean_id.starts_with("SLES") || clean_id.starts_with("SCES") {
            "EU"
        } else if clean_id.starts_with("SLPM") || clean_id.starts_with("SLPS") || clean_id.starts_with("SCPS") {
            "JP"
        } else {
            "US" // Default fallback
        };
        
        // GameTDB has reliable PS2 covers
        Some(format!(
            "https://art.gametdb.com/ps2/cover/{}/{}.jpg",
            region, clean_id
        ))
    }
    
    /// Get high-res cover from multiple sources
    pub async fn fetch_cover(game_id: &str, game_name: &str) -> Result<Vec<u8>> {
        // Try GameTDB first (has most PS2 covers)
        if let Some(url) = Self::get_ps2_cover_url(game_id) {
            debug!("Fetching cover from GameTDB: {}", url);
            if let Ok(response) = reqwest::get(&url).await {
                if response.status().is_success() {
                    if let Ok(bytes) = response.bytes().await {
                        info!("Successfully fetched cover for {}", game_id);
                        return Ok(bytes.to_vec());
                    }
                }
            }
        }
        
        // Fallback: Could add more sources here
        // - IGDB API (requires API key)
        // - TheGamesDB (requires API key)
        // - Local cache/database
        
        warn!("No cover found for {} ({})", game_id, game_name);
        Err(anyhow::anyhow!("Cover not found"))
    }
    
    /// Normalize game ID by removing prefixes and suffixes
    /// "BESLES-52056-HPA" -> "SLES-52056"
    fn normalize_game_id(game_id: &str) -> String {
        let mut id = game_id;
        
        // Remove BE/BA prefix
        if id.len() > 7 && id.chars().nth(2) == Some('S') {
            id = &id[2..];
        }
        
        // Remove any suffix after the main ID
        if let Some(dash_pos) = id.rfind('-') {
            if dash_pos > 4 { // Keep first dash, remove additional
                let parts: Vec<_> = id.split('-').collect();
                if parts.len() > 2 {
                    return format!("{}-{}", parts[0], parts[1]);
                }
            }
        }
        
        id.to_string()
    }
    
    /// Cache cover locally for offline access
    pub async fn cache_cover(game_id: &str, cover_data: &[u8]) -> Result<()> {
        use std::fs;
        use std::path::PathBuf;
        
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("retrosave")
            .join("covers")
            .join("ps2");
        
        fs::create_dir_all(&cache_dir)?;
        
        let file_path = cache_dir.join(format!("{}.jpg", Self::normalize_game_id(game_id)));
        fs::write(file_path, cover_data)?;
        
        Ok(())
    }
    
    /// Get cached cover if available
    pub fn get_cached_cover(game_id: &str) -> Option<Vec<u8>> {
        use std::fs;
        
        let cache_dir = dirs::cache_dir()?
            .join("retrosave")
            .join("covers")
            .join("ps2");
        
        let file_path = cache_dir.join(format!("{}.jpg", Self::normalize_game_id(game_id)));
        
        fs::read(file_path).ok()
    }
}

/// Metadata enrichment using game IDs
pub struct GameMetadataEnricher;

impl GameMetadataEnricher {
    /// Get additional game info that could be fetched using the game ID
    pub fn get_metadata_sources(game_id: &str) -> Vec<String> {
        let clean_id = GameCoverFetcher::normalize_game_id(game_id);
        
        vec![
            // Direct database lookups
            format!("https://psxdatacenter.com/psx2/games/{}.html", clean_id),
            format!("https://www.gametdb.com/PS2/{}", clean_id),
            
            // API endpoints (would need API keys)
            format!("https://api.igdb.com/v4/games?filter[game_id]={}", clean_id),
            format!("https://thegamesdb.net/api/GetGame.php?id={}", clean_id),
            
            // Cover art
            format!("https://art.gametdb.com/ps2/cover/US/{}.jpg", clean_id),
            format!("https://art.gametdb.com/ps2/disc/US/{}.png", clean_id),
            format!("https://art.gametdb.com/ps2/3D/US/{}.png", clean_id),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_normalize_game_id() {
        assert_eq!(
            GameCoverFetcher::normalize_game_id("BESLES-52056-HPA"),
            "SLES-52056"
        );
        
        assert_eq!(
            GameCoverFetcher::normalize_game_id("SLES-52056"),
            "SLES-52056"
        );
        
        assert_eq!(
            GameCoverFetcher::normalize_game_id("SCUS-97399"),
            "SCUS-97399"
        );
    }
    
    #[test]
    fn test_cover_url_generation() {
        assert_eq!(
            GameCoverFetcher::get_ps2_cover_url("SLES-52056"),
            Some("https://art.gametdb.com/ps2/cover/EU/SLES-52056.jpg".to_string())
        );
        
        assert_eq!(
            GameCoverFetcher::get_ps2_cover_url("SLUS-20946"),
            Some("https://art.gametdb.com/ps2/cover/US/SLUS-20946.jpg".to_string())
        );
    }
}