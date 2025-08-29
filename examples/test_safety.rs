use std::fs;
use retrosave::storage::ps2_memory_card::PS2MemoryCard;
use retrosave::storage::game_database::lookup_game_name;

fn main() {
    println!("PS2 Memory Card Safety Test");
    println!("============================\n");
    
    // Load your current memory card with multiple games
    let data = fs::read("/home/eralp/.var/app/net.pcsx2.PCSX2/config/PCSX2/memcards/1.ps2")
        .expect("Failed to read memory card");
    
    println!("Memory card size: {} bytes", data.len());
    
    if let Some(card) = PS2MemoryCard::new(data) {
        // Parse all saves
        let saves = card.parse_saves();
        println!("Total saves in memory card: {}\n", saves.len());
        
        // Generate metadata (as if Harry Potter was the current game)
        let metadata = card.generate_metadata("Harry Potter and the Philosopher's Stone".to_string());
        
        println!("=== Memory Card Metadata ===");
        println!("Primary game (triggered save): {}", metadata.primary_game);
        println!("Total saves: {}", metadata.total_saves);
        println!("Number of different games: {}", metadata.games_contained.len());
        println!("\n=== All Games in Memory Card ===");
        
        for game in &metadata.games_contained {
            println!("- {} ({}) - {} saves", 
                game.game_name, 
                game.game_id,
                game.save_count
            );
        }
        
        // Safety check simulation
        println!("\n=== Safety Check Simulation ===");
        
        // Check if we have Harry Potter
        let has_harry_potter = metadata.games_contained.iter()
            .any(|g| g.game_name.contains("Harry Potter"));
        
        if has_harry_potter {
            println!("✓ Harry Potter save exists");
        } else {
            println!("✗ No Harry Potter save found");
        }
        
        // Simulate what would happen if we restore from cloud
        if metadata.games_contained.len() > 1 {
            println!("\n⚠️  WARNING: Memory card contains {} different games!", 
                metadata.games_contained.len());
            println!("   Restoring from cloud would affect ALL these games:");
            
            for game in &metadata.games_contained {
                if !game.game_name.contains("Harry Potter") {
                    println!("   - {} would be OVERWRITTEN!", game.game_name);
                }
            }
            
            println!("\n   SAFETY: System should prevent automatic restore");
            println!("   ACTION: Ask user for confirmation before overwriting");
        } else {
            println!("\n✓ Safe to restore - only one game in memory card");
        }
        
        // Test game database
        println!("\n=== Game Database Test ===");
        let mut identified = 0;
        let mut unknown = 0;
        
        for (save_id, save) in &saves {
            if let Some(name) = lookup_game_name(&save.game_id) {
                println!("✓ {} → {}", save.game_id, name);
                identified += 1;
            } else {
                println!("? {} → Unknown game", save.game_id);
                unknown += 1;
            }
        }
        
        println!("\nDatabase coverage: {}/{} games identified ({:.0}%)",
            identified, 
            identified + unknown,
            (identified as f32 / (identified + unknown) as f32) * 100.0
        );
        
    } else {
        println!("Failed to parse memory card");
    }
}