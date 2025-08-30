use std::fs;
use retrosave::storage::ps2_memory_card::PS2MemoryCard;

fn main() {
    println!("Testing PS2 Memory Card Parser - Scanner Approach");
    println!("==================================================\n");
    
    let data = fs::read("/home/eralp/.var/app/net.pcsx2.PCSX2/config/PCSX2/memcards/1.ps2")
        .expect("Failed to read memory card");
    
    println!("Memory card size: {} bytes", data.len());
    
    if let Some(card) = PS2MemoryCard::new(data) {
        let saves = card.parse_saves();
        
        println!("Found {} saves in memory card:", saves.len());
        println!("---------------------------------");
        
        for (name, save) in &saves {
            println!("Save: {}", name);
            println!("  Game ID: {}", save.game_id);
            
            // Highlight Harry Potter saves
            if name.contains("52056") || name.contains("52055") {
                println!("  >>> THIS IS HARRY POTTER!");
            }
        }
        
        println!("---------------------------------");
        println!("\n=== Harry Potter Detection ===");
        println!("Has Harry Potter save: {}", card.has_harry_potter_save());
        
    } else {
        println!("Failed to load memory card - invalid format or size");
    }
}