use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::collections::HashMap;

fn main() {
    // Link X11 library for window title detection on Linux
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=X11");
    }
    
    // Generate game database from GameIndex.yaml
    println!("cargo:rerun-if-changed=../GameIndex.yaml");
    generate_game_database();
}

fn generate_game_database() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("game_database_generated.rs");
    
    // Read GameIndex.yaml
    let gameindex_path = "../GameIndex.yaml";
    let file = match File::open(gameindex_path) {
        Ok(f) => f,
        Err(_) => {
            println!("cargo:warning=GameIndex.yaml not found, using built-in database");
            return;
        }
    };
    
    let reader = BufReader::new(file);
    
    let mut games: HashMap<String, String> = HashMap::new();
    let mut current_id = String::new();
    let mut in_game_block = false;
    
    for line in reader.lines() {
        let line = line.unwrap();
        let trimmed = line.trim();
        
        // Check for game ID (e.g., "SLES-52056:")
        if trimmed.len() >= 10 && trimmed.ends_with(':') {
            if let Some(dash_pos) = trimmed.find('-') {
                if dash_pos == 4 || dash_pos == 5 {  // SLES-12345 or SCUS-12345
                    current_id = trimmed.trim_end_matches(':').to_string();
                    in_game_block = true;
                }
            }
        } else if in_game_block && trimmed.starts_with("name:") {
            // Extract game name
            let name = trimmed[5..].trim();
            // Remove quotes if present
            let name = name.trim_matches('"');
            if !games.contains_key(&current_id) {
                games.insert(current_id.clone(), name.to_string());
            }
        } else if in_game_block && trimmed.starts_with("name-en:") {
            // Use English name if available (overwrites Japanese name)
            let name = trimmed[8..].trim();
            let name = name.trim_matches('"');
            games.insert(current_id.clone(), name.to_string());
        } else if trimmed.is_empty() || trimmed.starts_with('#') {
            in_game_block = false;
        }
    }
    
    // Generate Rust code
    let mut output = File::create(&dest_path).unwrap();
    
    writeln!(output, "// Auto-generated from GameIndex.yaml").unwrap();
    writeln!(output, "// Total games: {}", games.len()).unwrap();
    writeln!(output).unwrap();
    writeln!(output, "use std::collections::HashMap;").unwrap();
    writeln!(output, "use once_cell::sync::Lazy;").unwrap();
    writeln!(output).unwrap();
    writeln!(output, "pub static GAME_DATABASE_GENERATED: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {{").unwrap();
    writeln!(output, "    let mut db = HashMap::new();").unwrap();
    writeln!(output).unwrap();
    
    // Sort games by ID for consistent output
    let mut sorted_games: Vec<_> = games.iter().collect();
    sorted_games.sort_by_key(|a| a.0);
    
    for (id, name) in sorted_games {
        // Escape the game name for Rust string literal
        let escaped_name = name
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        writeln!(output, "    db.insert(\"{id}\", \"{escaped_name}\");").unwrap();
    }
    
    writeln!(output).unwrap();
    writeln!(output, "    db").unwrap();
    writeln!(output, "}});").unwrap();
    
    println!("cargo:warning=Generated game database with {} games", games.len());
}