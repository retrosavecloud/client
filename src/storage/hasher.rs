use anyhow::Result;
use sha2::{Sha256, Digest};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use tracing::debug;

/// Calculate SHA256 hash of a file
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];
    
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    
    let result = hasher.finalize();
    let hash = format!("{:x}", result);
    
    debug!("Hashed file {:?}: {}", path, &hash[..8]);
    Ok(hash)
}

/// Get file size
pub fn get_file_size(path: &Path) -> Result<u64> {
    let metadata = std::fs::metadata(path)?;
    Ok(metadata.len())
}

/// Check if two files have the same hash
pub async fn has_file_changed(path: &Path, previous_hash: &str) -> Result<bool> {
    let current_hash = hash_file(path)?;
    Ok(current_hash != previous_hash)
}