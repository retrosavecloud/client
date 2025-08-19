use anyhow::{Result, Context};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use tracing::{debug, info};
use zstd::stream::{encode_all, decode_all};

#[derive(Debug, Clone, Copy)]
pub struct CompressionStats {
    pub original_size: u64,
    pub compressed_size: u64,
    pub compression_ratio: f32,
    pub compression_time_ms: u128,
}

impl CompressionStats {
    pub fn space_saved_percent(&self) -> f32 {
        if self.original_size == 0 {
            return 0.0;
        }
        ((self.original_size - self.compressed_size) as f32 / self.original_size as f32) * 100.0
    }
}

pub struct Compressor {
    compression_level: i32,
    enabled: bool,
}

impl Default for Compressor {
    fn default() -> Self {
        Self {
            compression_level: 3, // Default level, good balance of speed and compression
            enabled: true,
        }
    }
}

impl Compressor {
    pub fn new(compression_level: i32, enabled: bool) -> Self {
        // Clamp compression level between 1 and 22
        let level = compression_level.clamp(1, 22);
        Self {
            compression_level: level,
            enabled,
        }
    }
    
    pub fn set_level(&mut self, level: i32) {
        self.compression_level = level.clamp(1, 22);
    }
    
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    
    /// Compress a file and save it with .zst extension
    pub fn compress_file(&self, source_path: &Path, dest_path: &Path) -> Result<CompressionStats> {
        if !self.enabled {
            // If compression is disabled, just copy the file
            std::fs::copy(source_path, dest_path)
                .context("Failed to copy file")?;
            
            let size = std::fs::metadata(source_path)?.len();
            return Ok(CompressionStats {
                original_size: size,
                compressed_size: size,
                compression_ratio: 1.0,
                compression_time_ms: 0,
            });
        }
        
        let start = std::time::Instant::now();
        
        // Read source file
        let mut source_file = File::open(source_path)
            .context("Failed to open source file")?;
        let mut source_data = Vec::new();
        source_file.read_to_end(&mut source_data)
            .context("Failed to read source file")?;
        
        let original_size = source_data.len() as u64;
        
        // Compress data
        let compressed_data = encode_all(&source_data[..], self.compression_level)
            .context("Failed to compress data")?;
        
        let compressed_size = compressed_data.len() as u64;
        
        // Write compressed data to destination
        let mut dest_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(dest_path)
            .context("Failed to create destination file")?;
        
        dest_file.write_all(&compressed_data)
            .context("Failed to write compressed data")?;
        
        let compression_time_ms = start.elapsed().as_millis();
        let compression_ratio = compressed_size as f32 / original_size as f32;
        
        info!(
            "Compressed {} -> {} ({}% reduction, {:.2}x ratio) in {}ms",
            format_size(original_size),
            format_size(compressed_size),
            ((original_size - compressed_size) as f32 / original_size as f32 * 100.0) as u32,
            1.0 / compression_ratio,
            compression_time_ms
        );
        
        Ok(CompressionStats {
            original_size,
            compressed_size,
            compression_ratio,
            compression_time_ms,
        })
    }
    
    /// Decompress a .zst file
    pub fn decompress_file(&self, source_path: &Path, dest_path: &Path) -> Result<()> {
        // Check if file is actually compressed (has .zst extension)
        if !source_path.extension().map_or(false, |ext| ext == "zst") {
            // Not compressed, just copy
            std::fs::copy(source_path, dest_path)
                .context("Failed to copy file")?;
            return Ok(());
        }
        
        let start = std::time::Instant::now();
        
        // Read compressed file
        let mut source_file = File::open(source_path)
            .context("Failed to open compressed file")?;
        let mut compressed_data = Vec::new();
        source_file.read_to_end(&mut compressed_data)
            .context("Failed to read compressed file")?;
        
        // Decompress data
        let decompressed_data = decode_all(&compressed_data[..])
            .context("Failed to decompress data")?;
        
        // Write decompressed data
        let mut dest_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(dest_path)
            .context("Failed to create destination file")?;
        
        dest_file.write_all(&decompressed_data)
            .context("Failed to write decompressed data")?;
        
        debug!(
            "Decompressed {} -> {} in {}ms",
            format_size(compressed_data.len() as u64),
            format_size(decompressed_data.len() as u64),
            start.elapsed().as_millis()
        );
        
        Ok(())
    }
    
    /// Compress data in memory
    pub fn compress_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        if !self.enabled {
            return Ok(data.to_vec());
        }
        
        encode_all(data, self.compression_level)
            .context("Failed to compress data")
    }
    
    /// Decompress data in memory
    pub fn decompress_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        decode_all(data)
            .context("Failed to decompress data")
    }
}

/// Format bytes as human-readable size
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;
    
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    
    if unit_index == 0 {
        format!("{} {}", size as u64, UNITS[unit_index])
    } else {
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_compress_decompress_file() {
        let temp_dir = TempDir::new().unwrap();
        let source_path = temp_dir.path().join("test.txt");
        let compressed_path = temp_dir.path().join("test.txt.zst");
        let decompressed_path = temp_dir.path().join("test_restored.txt");
        
        // Create test file with repetitive content (compresses well)
        let test_content = "Retrosave saves your saves! ".repeat(1000);
        fs::write(&source_path, &test_content).unwrap();
        
        let compressor = Compressor::default();
        
        // Test compression
        let stats = compressor.compress_file(&source_path, &compressed_path).unwrap();
        assert!(stats.compressed_size < stats.original_size);
        assert!(stats.compression_ratio < 1.0);
        assert!(stats.space_saved_percent() > 0.0);
        
        // Test decompression
        compressor.decompress_file(&compressed_path, &decompressed_path).unwrap();
        
        // Verify content matches
        let restored_content = fs::read_to_string(&decompressed_path).unwrap();
        assert_eq!(restored_content, test_content);
    }
    
    #[test]
    fn test_compress_with_different_levels() {
        let temp_dir = TempDir::new().unwrap();
        let source_path = temp_dir.path().join("test.txt");
        
        // Create test file
        let test_content = "Test data for compression ".repeat(1000);
        fs::write(&source_path, &test_content).unwrap();
        
        // Test different compression levels
        let mut sizes = Vec::new();
        for level in [1, 3, 10, 22] {
            let compressed_path = temp_dir.path().join(format!("test_level_{}.zst", level));
            let compressor = Compressor::new(level, true);
            
            let stats = compressor.compress_file(&source_path, &compressed_path).unwrap();
            sizes.push((level, stats.compressed_size));
            
            println!("Level {}: {} bytes, ratio: {:.2}%, time: {}ms", 
                level, 
                stats.compressed_size,
                stats.space_saved_percent(),
                stats.compression_time_ms
            );
        }
        
        // Higher compression levels should generally produce smaller files
        // (though not always guaranteed for small files)
        assert!(sizes[3].1 <= sizes[0].1); // Level 22 should be <= level 1
    }
    
    #[test]
    fn test_compress_disabled() {
        let temp_dir = TempDir::new().unwrap();
        let source_path = temp_dir.path().join("test.txt");
        let dest_path = temp_dir.path().join("test_copy.txt");
        
        let test_content = "Test content";
        fs::write(&source_path, test_content).unwrap();
        
        let compressor = Compressor::new(3, false); // Disabled
        
        let stats = compressor.compress_file(&source_path, &dest_path).unwrap();
        assert_eq!(stats.original_size, stats.compressed_size);
        assert_eq!(stats.compression_ratio, 1.0);
        
        // Should just be a copy
        let dest_content = fs::read_to_string(&dest_path).unwrap();
        assert_eq!(dest_content, test_content);
    }
    
    #[test]
    fn test_memory_compression() {
        let compressor = Compressor::default();
        let data = b"This is test data that should compress well! ".repeat(100);
        
        let compressed = compressor.compress_data(&data).unwrap();
        assert!(compressed.len() < data.len());
        
        let decompressed = compressor.decompress_data(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }
    
    #[test]
    fn test_format_size() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(1048576), "1.00 MB");
        assert_eq!(format_size(8388608), "8.00 MB"); // PS2 memory card size
    }
}