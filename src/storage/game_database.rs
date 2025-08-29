use std::collections::HashMap;
use once_cell::sync::Lazy;

// Include the auto-generated database from build.rs
// This contains 12,823+ PS2 games parsed from PCSX2's GameIndex.yaml at compile time
// The generated file is located at: target/*/build/retrosave-*/out/game_database_generated.rs
// It's created automatically during build and NOT checked into version control
#[allow(dead_code)]
#[path = ""]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/game_database_generated.rs"));
}

/// FALLBACK PS2 Game Database - Only used if GameIndex.yaml is not available during build
/// 
/// The MAIN database comes from the auto-generated file above with 12,823+ games.
/// This hardcoded list below is just a small fallback with ~100 popular games
/// in case GameIndex.yaml is missing during compilation.
/// 
/// Priority order for game lookups:
/// 1. generated::GAME_DATABASE_GENERATED (12,823+ games from GameIndex.yaml)
/// 2. GAME_DATABASE (this fallback list with ~100 games)
pub static GAME_DATABASE: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();
    
    // Harry Potter games
    db.insert("SLES-52055", "Harry Potter and the Philosopher's Stone");
    db.insert("SLES-52056", "Harry Potter and the Philosopher's Stone");
    db.insert("SLUS-20826", "Harry Potter and the Philosopher's Stone");
    db.insert("SLPM-65465", "Harry Potter and the Philosopher's Stone");
    db.insert("SLPM-65650", "Harry Potter and the Philosopher's Stone");
    
    // God of War series
    db.insert("SCUS-97399", "God of War");
    db.insert("SCES-52950", "God of War");
    db.insert("SCUS-97481", "God of War II");
    db.insert("SCES-54206", "God of War II");
    
    // Grand Theft Auto series
    db.insert("SLUS-20062", "Grand Theft Auto III");
    db.insert("SLES-50330", "Grand Theft Auto III");
    db.insert("SLUS-20495", "Grand Theft Auto: Vice City");
    db.insert("SLES-51061", "Grand Theft Auto: Vice City");
    db.insert("SLUS-20946", "Grand Theft Auto: San Andreas");
    db.insert("SLES-52541", "Grand Theft Auto: San Andreas");
    
    // Final Fantasy series
    db.insert("SLUS-20312", "Final Fantasy X");
    db.insert("SCES-50490", "Final Fantasy X");
    db.insert("SLUS-20484", "Final Fantasy X-2");
    db.insert("SCES-51815", "Final Fantasy X-2");
    db.insert("SLUS-20965", "Final Fantasy XII");
    db.insert("SLES-54354", "Final Fantasy XII");
    
    // Metal Gear Solid series
    db.insert("SLUS-20144", "Metal Gear Solid 2: Sons of Liberty");
    db.insert("SLES-50383", "Metal Gear Solid 2: Sons of Liberty");
    db.insert("SLUS-20915", "Metal Gear Solid 3: Snake Eater");
    db.insert("SLES-52243", "Metal Gear Solid 3: Snake Eater");
    
    // Gran Turismo series
    db.insert("SCUS-97102", "Gran Turismo 3: A-Spec");
    db.insert("SCES-50294", "Gran Turismo 3: A-Spec");
    db.insert("SCUS-97328", "Gran Turismo 4");
    db.insert("SCES-51719", "Gran Turismo 4");
    
    // Kingdom Hearts series
    db.insert("SLUS-20370", "Kingdom Hearts");
    db.insert("SCES-50967", "Kingdom Hearts");
    db.insert("SLUS-21334", "Kingdom Hearts II");
    db.insert("SCES-54232", "Kingdom Hearts II");
    
    // Resident Evil series
    db.insert("SLUS-20669", "Resident Evil 4");
    db.insert("SLES-53702", "Resident Evil 4");
    db.insert("SLUS-20227", "Resident Evil Code: Veronica X");
    db.insert("SLES-50306", "Resident Evil Code: Veronica X");
    
    // Devil May Cry series
    db.insert("SLUS-20216", "Devil May Cry");
    db.insert("SLES-50358", "Devil May Cry");
    db.insert("SLUS-20627", "Devil May Cry 2");
    db.insert("SLES-51136", "Devil May Cry 2");
    db.insert("SLUS-20950", "Devil May Cry 3: Dante's Awakening");
    db.insert("SLES-53038", "Devil May Cry 3: Dante's Awakening");
    
    // Shadow of the Colossus / ICO
    db.insert("SCUS-97472", "Shadow of the Colossus");
    db.insert("SCES-53326", "Shadow of the Colossus");
    db.insert("SCUS-97113", "ICO");
    db.insert("SCES-50760", "ICO");
    
    // Silent Hill series
    db.insert("SLUS-20228", "Silent Hill 2");
    db.insert("SLES-50382", "Silent Hill 2");
    db.insert("SLUS-20622", "Silent Hill 3");
    db.insert("SLES-51434", "Silent Hill 3");
    db.insert("SLUS-20873", "Silent Hill 4: The Room");
    db.insert("SLES-52445", "Silent Hill 4: The Room");
    
    // Tekken series
    db.insert("SLUS-20001", "Tekken Tag Tournament");
    db.insert("SCES-50001", "Tekken Tag Tournament");
    db.insert("SLUS-20934", "Tekken 5");
    db.insert("SCES-53202", "Tekken 5");
    
    // Ratchet & Clank series
    db.insert("SCUS-97199", "Ratchet & Clank");
    db.insert("SCES-50916", "Ratchet & Clank");
    db.insert("SCUS-97268", "Ratchet & Clank: Going Commando");
    db.insert("SCES-51607", "Ratchet & Clank: Going Commando");
    db.insert("SCUS-97353", "Ratchet & Clank: Up Your Arsenal");
    db.insert("SCES-52456", "Ratchet & Clank: Up Your Arsenal");
    
    // Jak and Daxter series
    db.insert("SCUS-97124", "Jak and Daxter: The Precursor Legacy");
    db.insert("SCES-50361", "Jak and Daxter: The Precursor Legacy");
    db.insert("SCUS-97265", "Jak II");
    db.insert("SCES-51608", "Jak II");
    db.insert("SCUS-97330", "Jak 3");
    db.insert("SCES-52460", "Jak 3");
    
    // Sly Cooper series
    db.insert("SCUS-97198", "Sly Cooper and the Thievius Raccoonus");
    db.insert("SCES-50917", "Sly Cooper and the Thievius Raccoonus");
    db.insert("SCUS-97316", "Sly 2: Band of Thieves");
    db.insert("SCES-52529", "Sly 2: Band of Thieves");
    db.insert("SCUS-97464", "Sly 3: Honor Among Thieves");
    db.insert("SCES-53409", "Sly 3: Honor Among Thieves");
    
    // Spider-Man games
    db.insert("SLUS-20336", "Spider-Man");
    db.insert("SLES-50812", "Spider-Man");
    db.insert("SLUS-20776", "Spider-Man 2");
    db.insert("SLES-52372", "Spider-Man 2");
    
    // Popular sports games
    db.insert("SLUS-21955", "NBA 2K14");
    db.insert("SLES-52563", "FIFA 05");
    db.insert("SLUS-21434", "Madden NFL 08");
    
    // Other popular titles
    db.insert("SLUS-20552", "Mortal Kombat: Deadly Alliance");
    db.insert("SLES-51011", "Burnout 3: Takedown");
    db.insert("SLUS-20864", "Prince of Persia: The Sands of Time");
    db.insert("SLES-51508", "Need for Speed: Underground");
    db.insert("SLUS-21065", "Guitar Hero");
    db.insert("SLUS-20285", "Tony Hawk's Pro Skater 3");
    db.insert("SLUS-21307", "Crash Bandicoot: The Wrath of Cortex");
    db.insert("SLES-53624", "Black");
    db.insert("SLES-54203", "Okami");
    db.insert("SLES-55031", "Persona 4");
    
    db
});

/// Look up a game name by its ID
/// 
/// Searches in this order:
/// 1. First tries the generated database (12,823+ games from GameIndex.yaml)
/// 2. Falls back to hardcoded database if not found
/// 
/// Also handles ID normalization:
/// - Removes BE/BA prefixes: "BESLES-52056" -> "SLES-52056"
/// - Removes suffixes: "SLES-52056-HPA" -> "SLES-52056"
pub fn lookup_game_name(game_id: &str) -> Option<String> {
    // Remove any prefix like "BE" or "BA" from IDs like "BESLES-52056"
    let normalized_id = if game_id.len() > 7 && game_id.chars().nth(2) == Some('S') {
        // Skip BE/BA prefix: "BESLES-52056" -> "SLES-52056"
        &game_id[2..]
    } else {
        game_id
    };
    
    // Also try without any suffix: "SLES-52056-HPA" -> "SLES-52056"
    let base_id = normalized_id.split('-')
        .take(2)
        .collect::<Vec<_>>()
        .join("-");
    
    // Try the generated database first (12,000+ games)
    if let Some(name) = generated::GAME_DATABASE_GENERATED.get(base_id.as_str()) {
        return Some(name.to_string());
    }
    
    if let Some(name) = generated::GAME_DATABASE_GENERATED.get(normalized_id) {
        return Some(name.to_string());
    }
    
    if let Some(name) = generated::GAME_DATABASE_GENERATED.get(game_id) {
        return Some(name.to_string());
    }
    
    // Fallback to built-in database
    if let Some(name) = GAME_DATABASE.get(base_id.as_str()) {
        return Some(name.to_string());
    }
    
    if let Some(name) = GAME_DATABASE.get(normalized_id) {
        return Some(name.to_string());
    }
    
    if let Some(name) = GAME_DATABASE.get(game_id) {
        return Some(name.to_string());
    }
    
    None
}

/// Check if a game ID matches a specific game name
pub fn is_game_id_for_name(game_id: &str, game_name: &str) -> bool {
    if let Some(db_name) = lookup_game_name(game_id) {
        db_name.to_lowercase().contains(&game_name.to_lowercase())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_lookup_game_name() {
        assert_eq!(
            lookup_game_name("SLES-52056"),
            Some("Harry Potter and the Philosopher's Stone".to_string())
        );
        
        assert_eq!(
            lookup_game_name("BESLES-52056"),
            Some("Harry Potter and the Philosopher's Stone".to_string())
        );
        
        assert_eq!(
            lookup_game_name("BESLES-52056-HPA"),
            Some("Harry Potter and the Philosopher's Stone".to_string())
        );
        
        assert_eq!(
            lookup_game_name("SCUS-97399"),
            Some("God of War".to_string())
        );
        
        assert_eq!(lookup_game_name("UNKNOWN-12345"), None);
    }
}