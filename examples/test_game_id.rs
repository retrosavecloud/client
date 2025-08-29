use retrosave::storage::{Database, lookup_game_name};
use retrosave::storage::ps2_memory_card::PS2MemoryCard;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Testing Game ID Storage and Retrieval");
    println!("======================================\n");
    
    // Test 1: Game database lookup
    println!("1. Testing game database lookup:");
    let test_ids = vec![
        "SLES-52056",  // Harry Potter
        "SCUS-97399",  // God of War
        "SLUS-20946",  // GTA San Andreas
        "BESLES-52056-HPA",  // Harry Potter with prefix/suffix
    ];
    
    for id in test_ids {
        if let Some(name) = lookup_game_name(id) {
            println!("  ✓ {} -> {}", id, name);
        } else {
            println!("  ✗ {} -> Not found", id);
        }
    }
    
    // Test 2: Database storage
    println!("\n2. Testing database storage with game_id:");
    
    // Create a test database
    let db_path = std::env::temp_dir().join("test_game_id.db");
    let db = Database::new(Some(db_path.clone())).await?;
    
    // Create a game with game_id
    let game = db.get_or_create_game_with_id(
        "Harry Potter and the Philosopher's Stone",
        "PCSX2",
        Some("SLES-52056")
    ).await?;
    
    println!("  Created game:");
    println!("    - ID: {}", game.id);
    println!("    - Name: {}", game.name);
    println!("    - Emulator: {}", game.emulator);
    println!("    - Game ID: {:?}", game.game_id);
    
    // Retrieve the game and check game_id is persisted
    let retrieved_game = db.get_or_create_game("Harry Potter and the Philosopher's Stone", "PCSX2").await?;
    println!("\n  Retrieved game:");
    println!("    - Game ID: {:?}", retrieved_game.game_id);
    
    if retrieved_game.game_id == Some("SLES-52056".to_string()) {
        println!("  ✓ Game ID correctly persisted!");
    } else {
        println!("  ✗ Game ID not persisted correctly");
    }
    
    // Test 3: Memory card parsing
    println!("\n3. Testing memory card game ID extraction:");
    let mc_path = "/home/eralp/.var/app/net.pcsx2.PCSX2/config/PCSX2/memcards/1.ps2";
    if std::path::Path::new(mc_path).exists() {
        let data = std::fs::read(mc_path)?;
        if let Some(card) = PS2MemoryCard::new(data) {
            let saves = card.parse_saves();
            println!("  Found {} saves in memory card:", saves.len());
            
            for (_, save) in saves.iter().take(5) {
                let game_name = lookup_game_name(&save.game_id)
                    .unwrap_or_else(|| "Unknown".to_string());
                println!("    - {} -> {}", save.game_id, game_name);
            }
        }
    } else {
        println!("  Memory card not found at {}", mc_path);
    }
    
    // Test 4: Check all games in database
    println!("\n4. All games in database:");
    let all_games = db.get_all_games().await?;
    for game in all_games {
        println!("  - {} ({}): game_id = {:?}", game.name, game.emulator, game.game_id);
    }
    
    // Clean up
    let _ = std::fs::remove_file(db_path);
    
    println!("\n✅ All tests completed!");
    
    Ok(())
}