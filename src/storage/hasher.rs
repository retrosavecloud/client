use anyhow::Result;
use sha2::{Sha256, Digest};
use std::fs::File;
use std::io::Read;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_hash_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, b"Hello, World!").unwrap();

        let hash = hash_file(&file_path).unwrap();
        // SHA256 hash of "Hello, World!"
        assert_eq!(
            hash,
            "dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f"
        );
    }

    #[test]
    fn test_hash_file_empty() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("empty.txt");
        fs::write(&file_path, b"").unwrap();

        let hash = hash_file(&file_path).unwrap();
        // SHA256 hash of empty string
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_hash_file_large() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("large.bin");
        
        // Create a 10MB file with repeated pattern
        let pattern = b"RETROSAVE";
        let mut content = Vec::new();
        for _ in 0..(10 * 1024 * 1024 / pattern.len()) {
            content.extend_from_slice(pattern);
        }
        fs::write(&file_path, &content).unwrap();

        let hash = hash_file(&file_path).unwrap();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA256 is 64 hex characters
    }

    #[test]
    fn test_hash_file_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("nonexistent.txt");

        let result = hash_file(&file_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_file_size() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("sized.txt");
        let content = b"This is exactly 29 bytes long";
        fs::write(&file_path, content).unwrap();

        let size = get_file_size(&file_path).unwrap();
        assert_eq!(size, 29); // Verify exact byte count
    }

    #[test]
    fn test_get_file_size_empty() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("empty.txt");
        fs::write(&file_path, b"").unwrap();

        let size = get_file_size(&file_path).unwrap();
        assert_eq!(size, 0);
    }

    #[test]
    fn test_get_file_size_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("nonexistent.txt");

        let result = get_file_size(&file_path);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_has_file_changed_same() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, b"Test content").unwrap();

        let original_hash = hash_file(&file_path).unwrap();
        let changed = has_file_changed(&file_path, &original_hash).await.unwrap();
        assert!(!changed);
    }

    #[tokio::test]
    async fn test_has_file_changed_different() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, b"Original content").unwrap();

        let original_hash = hash_file(&file_path).unwrap();
        
        // Modify the file
        fs::write(&file_path, b"Modified content").unwrap();
        
        let changed = has_file_changed(&file_path, &original_hash).await.unwrap();
        assert!(changed);
    }

    #[tokio::test]
    async fn test_has_file_changed_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("nonexistent.txt");

        let result = has_file_changed(&file_path, "somehash").await;
        assert!(result.is_err());
    }
}