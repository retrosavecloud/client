use std::collections::HashMap;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use tracing::{debug, info, warn};

use crate::storage::ps2_memory_card::{PS2MemoryCard, PS2Save};
use retrosave_shared::MemoryCardMetadata;

/// Tracks changes in memory cards to detect which specific game was modified
pub struct MemoryCardTracker {
    /// Previous state of memory cards (stores the full card data for comparison)
    pub(crate) previous_states: HashMap<PathBuf, MemoryCardState>,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryCardState {
    /// The full memory card data for timestamp comparison
    pub(crate) card_data: Vec<u8>,
    /// All saves in the memory card
    pub(crate) saves: HashMap<String, PS2Save>,
    /// Hash of the entire file
    file_hash: String,
    /// Last check time
    last_checked: DateTime<Utc>,
    /// Metadata about all games
    metadata: MemoryCardMetadata,
}

impl MemoryCardTracker {
    pub fn new() -> Self {
        Self {
            previous_states: HashMap::new(),
        }
    }
    
    /// Simple synchronous update method that returns the changed game ID
    pub fn update(&mut self, path: &Path, data: &[u8]) -> Option<String> {
        // Calculate hash
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(data);
        let current_hash = format!("{:x}", hasher.finalize());
        
        // Parse current state
        let card = PS2MemoryCard::new(data.to_vec())?;
        let current_saves = card.parse_saves();
        
        // Get previous state
        let previous = self.previous_states.get(path);
        
        let result = if let Some(prev) = previous {
            // Compare with previous state
            if prev.file_hash == current_hash {
                None // No change
            } else {
                // Use the new detect_modified_save method for accurate detection
                if let Some(prev_card) = PS2MemoryCard::new(prev.card_data.clone()) {
                    if let Some(modified_save_name) = card.detect_modified_save(&prev_card) {
                        // Get the game ID for the modified save
                        current_saves.get(&modified_save_name)
                            .map(|save| save.game_id.clone())
                    } else {
                        // Couldn't detect specific change, use timestamp comparison
                        if let Some(last_modified) = card.get_last_modified_save() {
                            Some(last_modified.game_id)
                        } else {
                            None
                        }
                    }
                } else {
                    // Fallback to first game if we can't parse previous
                    current_saves.values().next().map(|s| s.game_id.clone())
                }
            }
        } else {
            // First time seeing this memory card
            // Use the most recently modified save if available
            if let Some(last_modified) = card.get_last_modified_save() {
                Some(last_modified.game_id)
            } else {
                // Fallback to first game found
                current_saves.values().next().map(|s| s.game_id.clone())
            }
        };
        
        // Update stored state
        if !current_saves.is_empty() {
            let metadata = card.generate_metadata("Unknown".to_string());
            self.previous_states.insert(path.to_path_buf(), MemoryCardState {
                card_data: data.to_vec(),
                saves: current_saves,
                file_hash: current_hash,
                last_checked: Utc::now(),
                metadata,
            });
        }
        
        result
    }
    
    /// Analyze a memory card and detect which game changed
    pub async fn detect_changed_game(
        &mut self,
        path: &Path,
        current_game: Option<&str>,
    ) -> Option<ChangedGame> {
        // Read current memory card
        let data = match tokio::fs::read(path).await {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to read memory card {}: {}", path.display(), e);
                return None;
            }
        };
        
        // Calculate hash
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let current_hash = format!("{:x}", hasher.finalize());
        
        // Parse current state (clone data since PS2MemoryCard takes ownership)
        let card = match PS2MemoryCard::new(data.clone()) {
            Some(c) => c,
            None => {
                warn!("Invalid PS2 memory card format: {}", path.display());
                return None;
            }
        };
        
        let current_saves = card.parse_saves();
        let current_metadata = card.generate_metadata(
            current_game.unwrap_or("Unknown").to_string()
        );
        
        // Get previous state
        let previous = self.previous_states.get(path);
        
        // Update stored state
        let current_state = MemoryCardState {
            card_data: data,  // Use the original data (no need to clone again)
            saves: current_saves.clone(),
            file_hash: current_hash.clone(),
            last_checked: Utc::now(),
            metadata: current_metadata.clone(),
        };
        
        let result = if let Some(prev) = previous {
            // Compare with previous state
            if prev.file_hash == current_hash {
                debug!("Memory card unchanged: {}", path.display());
                None
            } else {
                // Use the new detect_modified_save method for accurate detection
                if let Some(prev_card) = PS2MemoryCard::new(prev.card_data.clone()) {
                    if let Some(modified_save_name) = card.detect_modified_save(&prev_card) {
                        // Get the game info for the modified save
                        if let Some(save) = current_saves.get(&modified_save_name) {
                            let game_name = crate::storage::game_database::lookup_game_name(&save.game_id)
                                .unwrap_or_else(|| save.game_id.clone());
                            
                            info!("Detected save change for game '{}' in memory card", game_name);
                            
                            Some(ChangedGame {
                                game_id: save.game_id.clone(),
                                game_name,
                                change_type: ChangeType::Modified,
                            })
                        } else {
                            None
                        }
                    } else {
                        // Couldn't detect specific change via timestamps, use last modified
                        if let Some(last_modified) = card.get_last_modified_save() {
                            let game_name = crate::storage::game_database::lookup_game_name(&last_modified.game_id)
                                .unwrap_or_else(|| last_modified.game_id.clone());
                            
                            debug!("Using last modified save: {}", game_name);
                            
                            Some(ChangedGame {
                                game_id: last_modified.game_id.clone(),
                                game_name,
                                change_type: ChangeType::Modified,
                            })
                        } else {
                            debug!("Memory card changed but couldn't identify specific game");
                            None
                        }
                    }
                } else {
                    // Fallback if we can't parse previous state
                    self.find_changed_game(&prev.saves, &current_saves, current_game)
                }
            }
        } else {
            // First time seeing this memory card
            info!("New memory card detected: {}", path.display());
            
            // Use the most recently modified save
            if let Some(last_modified) = card.get_last_modified_save() {
                let game_name = crate::storage::game_database::lookup_game_name(&last_modified.game_id)
                    .unwrap_or_else(|| last_modified.game_id.clone());
                    
                Some(ChangedGame {
                    game_id: last_modified.game_id,
                    game_name,
                    change_type: ChangeType::Added,
                })
            } else if let Some(game) = current_game {
                // Fallback to current game hint
                Some(ChangedGame {
                    game_id: current_metadata.games_contained
                        .iter()
                        .find(|g| g.game_name.to_lowercase().contains(&game.to_lowercase()))
                        .map(|g| g.game_id.clone())
                        .unwrap_or_else(|| "UNKNOWN".to_string()),
                    game_name: game.to_string(),
                    change_type: ChangeType::Added,
                })
            } else {
                None
            }
        };
        
        // Store current state for next comparison
        self.previous_states.insert(path.to_path_buf(), current_state);
        
        result
    }
    
    /// Find which specific game changed between two states
    fn find_changed_game(
        &self,
        previous: &HashMap<String, PS2Save>,
        current: &HashMap<String, PS2Save>,
        hint_game: Option<&str>,
    ) -> Option<ChangedGame> {
        // If we have a hint (current game from window title), check it first
        if let Some(game_name) = hint_game {
            let game_lower = game_name.to_lowercase();
            
            // Check if any saves for this game changed
            for (save_id, current_save) in current {
                if current_save.name.to_lowercase().contains(&game_lower) ||
                   current_save.game_id.to_lowercase().contains(&game_lower) {
                    
                    // Check if this save is new or modified
                    if !previous.contains_key(save_id) {
                        return Some(ChangedGame {
                            game_id: current_save.game_id.clone(),
                            game_name: game_name.to_string(),
                            change_type: ChangeType::Added,
                        });
                    }
                }
            }
        }
        
        // Find any new saves
        for (save_id, save) in current {
            if !previous.contains_key(save_id) {
                // Try to get proper game name from database
                let game_name = crate::storage::game_database::lookup_game_name(&save.game_id)
                    .unwrap_or_else(|| save.game_id.clone());
                    
                return Some(ChangedGame {
                    game_id: save.game_id.clone(),
                    game_name,
                    change_type: ChangeType::Added,
                });
            }
        }
        
        // Find any removed saves
        for (save_id, save) in previous {
            if !current.contains_key(save_id) {
                let game_name = crate::storage::game_database::lookup_game_name(&save.game_id)
                    .unwrap_or_else(|| save.game_id.clone());
                    
                return Some(ChangedGame {
                    game_id: save.game_id.clone(),
                    game_name,
                    change_type: ChangeType::Removed,
                });
            }
        }
        
        // No specific game change detected
        None
    }
    
    /// Check if we should upload this memory card change
    pub fn should_upload(&self, changed_game: &Option<ChangedGame>, current_game: Option<&str>) -> bool {
        match changed_game {
            Some(change) => {
                // Only upload if the changed game matches the current game
                if let Some(current) = current_game {
                    let matches = change.game_name.to_lowercase().contains(&current.to_lowercase()) ||
                                 current.to_lowercase().contains(&change.game_name.to_lowercase());
                    
                    if !matches {
                        info!("Skipping upload: {} changed but current game is {}", 
                              change.game_name, current);
                    }
                    
                    matches
                } else {
                    // No current game known, upload any change
                    true
                }
            },
            None => {
                // No specific change detected, don't upload
                debug!("No specific game change detected, skipping upload");
                false
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChangedGame {
    pub game_id: String,
    pub game_name: String,
    pub change_type: ChangeType,
}

#[derive(Debug, Clone)]
pub enum ChangeType {
    Added,
    Modified,
    Removed,
}

impl Default for MemoryCardTracker {
    fn default() -> Self {
        Self::new()
    }
}