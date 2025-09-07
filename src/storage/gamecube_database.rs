/// GameCube game database for mapping game IDs to proper names
pub fn lookup_gamecube_game_name(game_id: &str) -> String {
    // Take first 4 characters for game code (excluding region and maker)
    let game_code = if game_id.len() >= 4 {
        &game_id[0..4]
    } else {
        game_id
    };
    
    // Map common GameCube games
    let name = match game_code {
        // Zelda games
        "GZLE" | "GZLP" | "GZLJ" => "The Legend of Zelda: The Wind Waker",
        "GZ2E" | "GZ2P" | "GZ2J" => "The Legend of Zelda: Twilight Princess",
        "GZWE" | "GZWP" | "GZWJ" => "The Legend of Zelda: Four Swords Adventures",
        "G4SE" | "G4SP" | "G4SJ" => "The Legend of Zelda: Ocarina of Time / Master Quest",
        
        // Mario games
        "GMSE" | "GMSP" | "GMSJ" => "Super Mario Sunshine",
        "GM4E" | "GM4P" | "GM4J" => "Mario Kart: Double Dash!!",
        "GMPE" | "GMPP" | "GMPJ" => "Mario Party 4",
        "GP5E" | "GP5P" | "GP5J" => "Mario Party 5",
        "GP6E" | "GP6P" | "GP6J" => "Mario Party 6",
        "GP7E" | "GP7P" | "GP7J" => "Mario Party 7",
        "G8ME" | "G8MP" | "G8MJ" => "Paper Mario: The Thousand-Year Door",
        
        // Smash Bros
        "GALE" | "GALP" | "GALJ" => "Super Smash Bros. Melee",
        
        // Metroid
        "GM8E" | "GM8P" | "GM8J" => "Metroid Prime",
        "G2ME" | "G2MP" | "G2MJ" => "Metroid Prime 2: Echoes",
        
        // Animal Crossing
        "GAFE" | "GAFP" | "GAFJ" => "Animal Crossing",
        
        // F-Zero
        "GFZE" | "GFZP" | "GFZJ" => "F-Zero GX",
        
        // Star Fox
        "GFSE" | "GFSP" => "Star Fox Adventures",
        "GFAE" | "GFAP" => "Star Fox: Assault",
        
        // Pikmin
        "GPIE" | "GPIP" | "GPIJ" => "Pikmin",
        "GPVE" | "GPVP" | "GPVJ" => "Pikmin 2",
        
        // Luigi's Mansion
        "GLME" | "GLMP" | "GLMJ" => "Luigi's Mansion",
        
        // Resident Evil
        "GBIE" | "GBIP" | "GBIJ" => "Resident Evil",
        "GHUE" | "GHUP" | "GHUJ" => "Resident Evil Zero",
        "G4BE" | "G4BP" | "G4BJ" => "Resident Evil 4",
        
        // Sonic games
        "GXSE" | "GXSP" => "Sonic Adventure 2: Battle",
        "GSNE" | "GSNP" | "GSNJ" => "Sonic Adventure DX: Director's Cut",
        "G9SE" | "G9SP" => "Sonic Heroes",
        
        // Tales series
        "GTOE" | "GTOP" | "GTOJ" => "Tales of Symphonia",
        
        // Fire Emblem
        "GFEE" | "GFEP" | "GFEJ" => "Fire Emblem: Path of Radiance",
        
        // Kirby
        "GKYE" | "GKYP" | "GKYJ" => "Kirby Air Ride",
        
        // Pokemon
        "GXXE" | "GXXP" => "Pokemon XD: Gale of Darkness",
        "GC6E" | "GC6P" | "GC6J" => "Pokemon Colosseum",
        
        // Metal Gear Solid
        "GGSEA" | "GGSPA" | "GGSJA" => "Metal Gear Solid: The Twin Snakes",
        
        // Final Fantasy
        "GCCE" | "GCCP" | "GCCJ" => "Final Fantasy Crystal Chronicles",
        
        // Viewtiful Joe
        "GVJE" | "GVJP" | "GVJJ" => "Viewtiful Joe",
        "G2VE" | "G2VP" | "G2VJ" => "Viewtiful Joe 2",
        
        // Other popular games
        "GTEE" | "GTEP" | "GTEJ" => "Eternal Darkness: Sanity's Requiem",
        "GGOE" | "GGOP" => "GoldenEye: Rogue Agent",
        "GIKE" | "GIKP" | "GIKJ" => "Ikaruga",
        "GRHE" | "GRHP" | "GRHJ" => "Rogue Squadron III: Rebel Strike",
        "GRSE" | "GRSP" | "GRSJ" => "Rogue Squadron II: Rogue Leader",
        "GSWE" | "GSWP" | "GSWJ" => "Star Wars: The Clone Wars",
        "GW2E" | "GW2P" | "GW2J" => "WarioWare, Inc.: Mega Party Game$!",
        "GWWE" | "GWWP" | "GWWJ" => "WarioWorld",
        "GUME" | "GUMP" | "GUMJ" => "Super Monkey Ball",
        "GM2E" | "GM2P" | "GM2J" => "Super Monkey Ball 2",
        "GHQE" | "GHQP" => "Harvest Moon: A Wonderful Life",
        "GKBE" | "GKBP" | "GKBJ" => "Baten Kaitos: Eternal Wings and the Lost Ocean",
        "GK2E" | "GK2J" => "Baten Kaitos Origins",
        "GSAE" | "GSAP" | "GSAJ" => "Sonic Adventure 2: Battle",
        
        // Default: return the game ID if not found
        _ => return format!("GameCube Game ({})", game_id)
    };
    
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_gamecube_game_lookup() {
        assert_eq!(
            lookup_gamecube_game_name("GZLE01"),
            "The Legend of Zelda: The Wind Waker"
        );
        
        assert_eq!(
            lookup_gamecube_game_name("GALE01"),
            "Super Smash Bros. Melee"
        );
        
        assert_eq!(
            lookup_gamecube_game_name("GAFE01"),
            "Animal Crossing"
        );
        
        assert_eq!(
            lookup_gamecube_game_name("UNKNOWN"),
            "GameCube Game (UNKNOWN)"
        );
    }
}