mod common;

use anyhow::Result;
use tempfile::TempDir;
use tokio::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

extern crate retrosave;
use retrosave::storage::{Database, SaveWatcher};
use retrosave::monitor::{MonitorEvent, SaveResult};

#[tokio::test]
async fn test_save_detection_workflow() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let save_dir = temp_dir.path().join("saves");
    std::fs::create_dir_all(&save_dir)?;
    
    let db_path = temp_dir.path().join("test.db");
    let db = Arc::new(Database::new(Some(db_path)).await?);
    
    // Create a save watcher
    let (mut watcher, mut save_receiver) = SaveWatcher::new(save_dir.clone(), db.clone())?;
    
    // Start the watcher
    watcher.start().await?;
    
    // Create a test save file
    let save_file = save_dir.join("test_game.ps2");
    std::fs::write(&save_file, b"initial save data")?;
    
    // Wait a bit for the watcher to detect the file
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Modify the save file
    std::fs::write(&save_file, b"modified save data")?;
    
    // Check for changes manually
    let changes = watcher.check_for_changes().await?;
    assert!(changes > 0, "Should detect save file change");
    
    // Clean up
    watcher.stop();
    
    Ok(())
}

#[tokio::test]
async fn test_manual_save_trigger() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    let db = Arc::new(Database::new(Some(db_path)).await?);
    
    // Create channels for monitoring
    let (event_sender, mut event_receiver) = mpsc::channel(100);
    let (cmd_sender, cmd_receiver) = mpsc::channel(10);
    
    // Start monitoring in background
    let db_clone = db.clone();
    let monitor_handle = tokio::spawn(async move {
        retrosave::monitor::start_monitoring_with_commands(
            event_sender,
            db_clone,
            cmd_receiver
        ).await
    });
    
    // Send manual save command
    cmd_sender.send(retrosave::monitor::MonitorCommand::TriggerManualSave).await?;
    
    // Wait for response
    let timeout = tokio::time::timeout(Duration::from_secs(1), async {
        while let Some(event) = event_receiver.recv().await {
            if let MonitorEvent::ManualSaveResult(result) = event {
                match result {
                    SaveResult::NoChanges => return Ok(()),
                    SaveResult::Failed(msg) => {
                        // Expected when no emulator is running
                        assert!(msg.contains("No emulator running"));
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
        Err(anyhow::anyhow!("No save result received"))
    }).await;
    
    // Clean up
    monitor_handle.abort();
    
    timeout??;
    Ok(())
}

#[tokio::test]
async fn test_database_persistence() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    
    // Create database and add data
    {
        let db = Database::new(Some(db_path.clone())).await?;
        
        // Add a game and saves
        let game = db.get_or_create_game("Persistence Test", "PCSX2").await?;
        db.record_save(game.id, "/save1.dat", "hash1", 1024, None).await?;
        db.record_save(game.id, "/save2.dat", "hash2", 2048, None).await?;
        
        // Add settings
        db.set_setting("test_key", "test_value").await?;
    }
    
    // Reopen database and verify data persists
    {
        let db = Database::new(Some(db_path)).await?;
        
        // Check game exists
        let game = db.get_or_create_game("Persistence Test", "PCSX2").await?;
        assert_eq!(game.total_saves, 2);
        
        // Check saves exist
        let saves = db.get_saves_for_game(game.id, None).await?;
        assert_eq!(saves.len(), 2);
        
        // Check settings exist
        let value = db.get_setting("test_key").await?;
        assert_eq!(value, Some("test_value".to_string()));
        
        // Check stats
        let (games, total_saves) = db.get_stats().await?;
        assert_eq!(games, 1);
        assert_eq!(total_saves, 2);
    }
    
    Ok(())
}

#[tokio::test]
async fn test_save_versioning() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    let db = Arc::new(Database::new(Some(db_path)).await?);
    
    let game = db.get_or_create_game("Version Test", "PCSX2").await?;
    
    // Record multiple saves
    for i in 1..=10 {
        db.record_save(
            game.id,
            &format!("/save{}.dat", i),
            &format!("hash{}", i),
            1024 * i,
            None
        ).await?;
    }
    
    // Verify versions are assigned correctly
    let saves = db.get_saves_for_game(game.id, None).await?;
    assert_eq!(saves.len(), 10);
    
    // Clean up old saves (keep only 5)
    let deleted = db.cleanup_old_saves(game.id, 5).await?;
    assert_eq!(deleted.len(), 5);
    
    // Verify only recent saves remain
    let remaining = db.get_saves_for_game(game.id, None).await?;
    assert_eq!(remaining.len(), 5);
    
    // The newest saves should be kept (versions 6-10)
    let min_version = remaining.iter().map(|s| s.version).min().unwrap();
    assert_eq!(min_version, 6);
    
    Ok(())
}

#[tokio::test]
async fn test_multiple_games_tracking() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    let db = Arc::new(Database::new(Some(db_path)).await?);
    
    // Create multiple games
    let game1 = db.get_or_create_game("Final Fantasy X", "PCSX2").await?;
    let game2 = db.get_or_create_game("Metal Gear Solid 3", "PCSX2").await?;
    let game3 = db.get_or_create_game("Super Mario Sunshine", "Dolphin").await?;
    
    // Add saves for each game
    db.record_save(game1.id, "/ff10_save.ps2", "hash1", 8192, None).await?;
    db.record_save(game2.id, "/mgs3_save.ps2", "hash2", 8192, None).await?;
    db.record_save(game3.id, "/mario_save.gci", "hash3", 4096, None).await?;
    
    // Add more saves for game1
    db.record_save(game1.id, "/ff10_save2.ps2", "hash4", 8192, None).await?;
    db.record_save(game1.id, "/ff10_save3.ps2", "hash5", 8192, None).await?;
    
    // Verify each game's saves
    let game1_saves = db.get_saves_for_game(game1.id, None).await?;
    assert_eq!(game1_saves.len(), 3);
    
    let game2_saves = db.get_saves_for_game(game2.id, None).await?;
    assert_eq!(game2_saves.len(), 1);
    
    let game3_saves = db.get_saves_for_game(game3.id, None).await?;
    assert_eq!(game3_saves.len(), 1);
    
    // Verify all games are tracked
    let all_games = db.get_all_games().await?;
    assert_eq!(all_games.len(), 3);
    
    // Verify total stats
    let (total_games, total_saves) = db.get_stats().await?;
    assert_eq!(total_games, 3);
    assert_eq!(total_saves, 5);
    
    Ok(())
}