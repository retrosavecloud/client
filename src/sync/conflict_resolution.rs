use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, warn};

/// Represents a conflict between local and cloud saves
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveConflict {
    pub game_id: String,
    pub game_name: String,
    pub local_version: SaveVersion,
    pub cloud_version: SaveVersion,
    pub conflict_type: ConflictType,
    pub recommended_action: ResolutionAction,
}

/// Information about a save version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveVersion {
    pub timestamp: DateTime<Utc>,
    pub size: u64,
    pub hash: String,
    pub save_count: usize,  // For memory cards - number of saves
    pub device_name: Option<String>,
}

/// Type of conflict detected
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConflictType {
    /// Both versions have been modified
    BothModified,
    /// Local is newer
    LocalNewer,
    /// Cloud is newer
    CloudNewer,
    /// Same timestamp but different content
    SameTimeButDifferent,
    /// Local game doesn't exist in cloud memory card
    LocalOnly,
    /// Cloud game doesn't exist in local memory card
    CloudOnly,
}

/// Possible resolution actions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResolutionAction {
    /// Use local version
    KeepLocal,
    /// Use cloud version
    UseCloud,
    /// Merge both (for memory cards - keep newer of each game)
    Merge,
    /// Skip this save
    Skip,
    /// Ask user
    AskUser,
}

/// Result of conflict resolution
#[derive(Debug, Clone)]
pub struct ResolutionResult {
    pub action_taken: ResolutionAction,
    pub games_kept_local: Vec<String>,
    pub games_kept_cloud: Vec<String>,
    pub games_merged: Vec<String>,
    pub games_skipped: Vec<String>,
}

/// Memory card conflict analyzer
pub struct ConflictAnalyzer;

impl ConflictAnalyzer {
    /// Analyze conflicts between local and cloud memory cards
    pub fn analyze_memory_card_conflicts(
        local_metadata: &retrosave_shared::MemoryCardMetadata,
        cloud_metadata: &retrosave_shared::MemoryCardMetadata,
        local_hash: &str,
        cloud_hash: &str,
        local_timestamp: DateTime<Utc>,
        cloud_timestamp: DateTime<Utc>,
    ) -> Vec<SaveConflict> {
        let mut conflicts = Vec::new();
        
        // Build maps for easy lookup
        let mut local_games: HashMap<String, &retrosave_shared::GameInfo> = HashMap::new();
        for game in &local_metadata.games_contained {
            local_games.insert(game.game_id.clone(), game);
        }
        
        let mut cloud_games: HashMap<String, &retrosave_shared::GameInfo> = HashMap::new();
        for game in &cloud_metadata.games_contained {
            cloud_games.insert(game.game_id.clone(), game);
        }
        
        // Check each game for conflicts
        let all_game_ids: std::collections::HashSet<String> = 
            local_games.keys().chain(cloud_games.keys()).cloned().collect();
        
        for game_id in all_game_ids {
            let local_game = local_games.get(&game_id);
            let cloud_game = cloud_games.get(&game_id);
            
            match (local_game, cloud_game) {
                (Some(local), Some(cloud)) => {
                    // Game exists in both - check for conflicts
                    if local.save_count != cloud.save_count {
                        let conflict_type = if local_timestamp > cloud_timestamp {
                            ConflictType::LocalNewer
                        } else if cloud_timestamp > local_timestamp {
                            ConflictType::CloudNewer
                        } else {
                            ConflictType::SameTimeButDifferent
                        };
                        
                        let recommended = Self::recommend_action(&conflict_type, local_timestamp, cloud_timestamp);
                        
                        conflicts.push(SaveConflict {
                            game_id: game_id.clone(),
                            game_name: local.game_name.clone(),
                            local_version: SaveVersion {
                                timestamp: local_timestamp,
                                size: 0, // We don't track individual game sizes
                                hash: local_hash.to_string(),
                                save_count: local.save_count,
                                device_name: None,
                            },
                            cloud_version: SaveVersion {
                                timestamp: cloud_timestamp,
                                size: 0,
                                hash: cloud_hash.to_string(),
                                save_count: cloud.save_count,
                                device_name: None,
                            },
                            conflict_type,
                            recommended_action: recommended,
                        });
                    }
                }
                (Some(local), None) => {
                    // Game only exists locally
                    conflicts.push(SaveConflict {
                        game_id: game_id.clone(),
                        game_name: local.game_name.clone(),
                        local_version: SaveVersion {
                            timestamp: local_timestamp,
                            size: 0,
                            hash: local_hash.to_string(),
                            save_count: local.save_count,
                            device_name: None,
                        },
                        cloud_version: SaveVersion {
                            timestamp: cloud_timestamp,
                            size: 0,
                            hash: cloud_hash.to_string(),
                            save_count: 0,
                            device_name: None,
                        },
                        conflict_type: ConflictType::LocalOnly,
                        recommended_action: ResolutionAction::KeepLocal,
                    });
                }
                (None, Some(cloud)) => {
                    // Game only exists in cloud
                    conflicts.push(SaveConflict {
                        game_id: game_id.clone(),
                        game_name: cloud.game_name.clone(),
                        local_version: SaveVersion {
                            timestamp: local_timestamp,
                            size: 0,
                            hash: local_hash.to_string(),
                            save_count: 0,
                            device_name: None,
                        },
                        cloud_version: SaveVersion {
                            timestamp: cloud_timestamp,
                            size: 0,
                            hash: cloud_hash.to_string(),
                            save_count: cloud.save_count,
                            device_name: None,
                        },
                        conflict_type: ConflictType::CloudOnly,
                        recommended_action: ResolutionAction::UseCloud,
                    });
                }
                (None, None) => unreachable!(),
            }
        }
        
        conflicts
    }
    
    /// Recommend an action based on conflict type and timestamps
    fn recommend_action(
        conflict_type: &ConflictType,
        local_time: DateTime<Utc>,
        cloud_time: DateTime<Utc>,
    ) -> ResolutionAction {
        match conflict_type {
            ConflictType::LocalNewer => {
                // If local is significantly newer (>1 hour), keep local
                if local_time.signed_duration_since(cloud_time).num_hours() > 1 {
                    ResolutionAction::KeepLocal
                } else {
                    ResolutionAction::AskUser
                }
            }
            ConflictType::CloudNewer => {
                // If cloud is significantly newer (>1 hour), use cloud
                if cloud_time.signed_duration_since(local_time).num_hours() > 1 {
                    ResolutionAction::UseCloud
                } else {
                    ResolutionAction::AskUser
                }
            }
            ConflictType::BothModified => ResolutionAction::AskUser,
            ConflictType::SameTimeButDifferent => ResolutionAction::AskUser,
            ConflictType::LocalOnly => ResolutionAction::KeepLocal,
            ConflictType::CloudOnly => ResolutionAction::UseCloud,
        }
    }
    
    /// Apply resolution strategy to conflicts
    pub fn resolve_conflicts(
        conflicts: &[SaveConflict],
        strategy: ResolutionStrategy,
    ) -> ResolutionResult {
        let mut games_kept_local = Vec::new();
        let mut games_kept_cloud = Vec::new();
        let mut games_merged = Vec::new();
        let mut games_skipped = Vec::new();
        
        for conflict in conflicts {
            let action = match strategy {
                ResolutionStrategy::AlwaysLocal => ResolutionAction::KeepLocal,
                ResolutionStrategy::AlwaysCloud => ResolutionAction::UseCloud,
                ResolutionStrategy::AlwaysNewer => {
                    if conflict.local_version.timestamp > conflict.cloud_version.timestamp {
                        ResolutionAction::KeepLocal
                    } else {
                        ResolutionAction::UseCloud
                    }
                }
                ResolutionStrategy::Interactive => conflict.recommended_action.clone(),
                ResolutionStrategy::Smart => conflict.recommended_action.clone(),
            };
            
            match action {
                ResolutionAction::KeepLocal => {
                    games_kept_local.push(conflict.game_name.clone());
                    info!("Keeping local version of {}", conflict.game_name);
                }
                ResolutionAction::UseCloud => {
                    games_kept_cloud.push(conflict.game_name.clone());
                    info!("Using cloud version of {}", conflict.game_name);
                }
                ResolutionAction::Merge => {
                    games_merged.push(conflict.game_name.clone());
                    info!("Merging versions of {}", conflict.game_name);
                }
                ResolutionAction::Skip => {
                    games_skipped.push(conflict.game_name.clone());
                    warn!("Skipping {}", conflict.game_name);
                }
                ResolutionAction::AskUser => {
                    // In non-interactive mode, default to keeping newer
                    if conflict.local_version.timestamp > conflict.cloud_version.timestamp {
                        games_kept_local.push(conflict.game_name.clone());
                    } else {
                        games_kept_cloud.push(conflict.game_name.clone());
                    }
                }
            }
        }
        
        ResolutionResult {
            action_taken: ResolutionAction::Merge,
            games_kept_local,
            games_kept_cloud,
            games_merged,
            games_skipped,
        }
    }
}

/// Strategy for resolving conflicts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResolutionStrategy {
    /// Always keep local version
    AlwaysLocal,
    /// Always use cloud version
    AlwaysCloud,
    /// Always use newer version
    AlwaysNewer,
    /// Ask user for each conflict
    Interactive,
    /// Use smart heuristics
    Smart,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_conflict_detection() {
        let local_metadata = retrosave_shared::MemoryCardMetadata {
            games_contained: vec![
                retrosave_shared::GameInfo {
                    game_id: "SLES-52056".to_string(),
                    game_name: "Harry Potter".to_string(),
                    save_count: 3,
                },
            ],
            primary_game: "Harry Potter".to_string(),
            total_saves: 3,
            format_version: "1.0".to_string(),
        };
        
        let cloud_metadata = retrosave_shared::MemoryCardMetadata {
            games_contained: vec![
                retrosave_shared::GameInfo {
                    game_id: "SLES-52056".to_string(),
                    game_name: "Harry Potter".to_string(),
                    save_count: 2,
                },
            ],
            primary_game: "Harry Potter".to_string(),
            total_saves: 2,
            format_version: "1.0".to_string(),
        };
        
        let conflicts = ConflictAnalyzer::analyze_memory_card_conflicts(
            &local_metadata,
            &cloud_metadata,
            "local_hash",
            "cloud_hash",
            Utc::now(),
            Utc::now() - chrono::Duration::hours(2),
        );
        
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, ConflictType::LocalNewer);
    }
}