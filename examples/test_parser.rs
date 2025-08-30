use std::fs;

fn main() {
    println!("Testing PS2 Memory Card Parser with PCSX2 format knowledge");
    println!("============================================================\n");
    
    let data = fs::read("/home/eralp/.var/app/net.pcsx2.PCSX2/config/PCSX2/memcards/1.ps2")
        .expect("Failed to read memory card");
    
    println!("Memory card size: {} bytes", data.len());
    println!("Memory card type: {}", if data.len() == 8650752 { "With ECC" } else { "Without ECC" });
    
    // Check header
    let header = String::from_utf8_lossy(&data[0..28]);
    println!("Header: '{}'", header);
    
    // Read superblock to get root directory cluster
    let root_cluster = u32::from_le_bytes([
        data[0x124],
        data[0x125],
        data[0x126],
        data[0x127],
    ]);
    println!("Root cluster from superblock: {}", root_cluster);
    
    // Calculate directory offset
    let dir_offset = 0x2000 + (root_cluster as usize * 0x200);
    println!("Directory starts at offset: 0x{:x}\n", dir_offset);
    
    println!("=== Directory Entries ===");
    
    let mut found_harry_potter = false;
    
    // Read directory entries
    for i in 0..16 {
        let entry_offset = dir_offset + (i * 0x200);
        
        if entry_offset + 0x200 > data.len() {
            break;
        }
        
        let entry = &data[entry_offset..entry_offset + 0x200];
        
        // Read mode flags (first 4 bytes)
        let mode = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
        
        // Skip empty entries
        if mode == 0 {
            continue;
        }
        
        // Name is at offset 0x40
        let name_bytes = &entry[0x40..0x60];
        let name = String::from_utf8_lossy(name_bytes)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        
        println!("Entry {}: mode=0x{:08x}", i, mode);
        println!("  Name: '{}'", name);
        println!("  Is Directory: {}", (mode & 0x0010) != 0);
        
        // Check if this might be Harry Potter
        let name_lower = name.to_lowercase();
        if name_lower.contains("harry") || name_lower.contains("potter") || 
           name_lower.contains("hp") || name_lower.contains("sles-520") ||
           name_lower.contains("slus-208") || name_lower.contains("slpm-654") ||
           name_lower.contains("besles-52055") || name_lower.contains("besles-52056") {
            println!("  >>> FOUND HARRY POTTER SAVE!");
            found_harry_potter = true;
        }
        
        // Show first few bytes as hex for debugging
        print!("  First 16 bytes: ");
        for j in 0..16 {
            print!("{:02x} ", entry[j]);
        }
        println!("\n");
    }
    
    println!("=================================");
    println!("Harry Potter save found: {}", found_harry_potter);
}