use anyhow::Result;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use std::fs;
use std::io::Write;

/// Test utilities for creating temporary test environments
pub struct TestEnvironment {
    pub temp_dir: TempDir,
    pub save_dir: PathBuf,
    pub db_path: PathBuf,
}

impl TestEnvironment {
    /// Create a new test environment with temporary directories
    pub fn new() -> Result<Self> {
        let temp_dir = TempDir::new()?;
        let save_dir = temp_dir.path().join("saves");
        let db_path = temp_dir.path().join("test.db");
        
        fs::create_dir_all(&save_dir)?;
        
        Ok(Self {
            temp_dir,
            save_dir,
            db_path,
        })
    }
    
    /// Create a test save file with specific content
    pub fn create_save_file(&self, name: &str, content: &[u8]) -> Result<PathBuf> {
        let file_path = self.save_dir.join(name);
        let mut file = fs::File::create(&file_path)?;
        file.write_all(content)?;
        Ok(file_path)
    }
    
    /// Create a PCSX2 memory card file (.ps2)
    pub fn create_memory_card(&self, name: &str) -> Result<PathBuf> {
        let file_name = format!("{}.ps2", name);
        // Memory cards are typically 8MB
        let content = vec![0u8; 8 * 1024 * 1024];
        self.create_save_file(&file_name, &content)
    }
    
    /// Create a PCSX2 save state file (.p2s)
    pub fn create_save_state(&self, name: &str) -> Result<PathBuf> {
        let file_name = format!("{}.p2s", name);
        // Save states vary in size, use a smaller test size
        let content = vec![0u8; 1024 * 1024];
        self.create_save_file(&file_name, &content)
    }
    
    /// Modify a file's content to simulate a save change
    pub fn modify_file(&self, path: &Path, new_content: &[u8]) -> Result<()> {
        let mut file = fs::File::create(path)?;
        file.write_all(new_content)?;
        Ok(())
    }
}

/// Mock process info for testing emulator detection
pub struct MockProcess {
    pub name: String,
    pub pid: u32,
    pub exe_path: String,
    pub cmd_args: Vec<String>,
}

impl MockProcess {
    pub fn pcsx2() -> Self {
        Self {
            name: "pcsx2".to_string(),
            pid: 1234,
            exe_path: "/usr/bin/pcsx2".to_string(),
            cmd_args: vec!["--game".to_string(), "test.iso".to_string()],
        }
    }
    
    pub fn pcsx2_with_game(game_path: &str) -> Self {
        Self {
            name: "pcsx2".to_string(),
            pid: 1234,
            exe_path: "/usr/bin/pcsx2".to_string(),
            cmd_args: vec![game_path.to_string()],
        }
    }
}

/// Helper to create a test database
pub async fn create_test_database(path: &Path) -> Result<retrosave::storage::Database> {
    retrosave::storage::Database::new(Some(path.to_path_buf())).await
}

/// Helper to wait for async operations with timeout
pub async fn wait_for_condition<F>(condition: F, timeout_ms: u64) -> bool
where
    F: Fn() -> bool,
{
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);
    
    while start.elapsed() < timeout {
        if condition() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    
    false
}