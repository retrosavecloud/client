use anyhow::{Result, Context};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use tracing::{info, debug, error};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: i64,
    pub name: String,
    pub emulator: String,
    pub path: Option<String>,
    pub last_played: Option<DateTime<Utc>>,
    pub total_saves: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Save {
    pub id: i64,
    pub game_id: i64,
    pub timestamp: DateTime<Utc>,
    pub file_path: String,
    pub file_hash: String,
    pub file_size: i64,
    pub version: i32,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub value: String,
}

pub struct Database {
    pool: SqlitePool,
    db_path: PathBuf,
}

impl Database {
    /// Create a new database connection
    pub async fn new(db_path: Option<PathBuf>) -> Result<Self> {
        // Use provided path or default to user data directory
        let db_path = db_path.unwrap_or_else(|| {
            let dirs = directories::ProjectDirs::from("com", "retrosave", "retrosave")
                .expect("Failed to get project directories");
            let mut path = dirs.data_dir().to_path_buf();
            path.push("retrosave.db");
            path
        });

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create database directory")?;
        }

        info!("Opening database at: {:?}", db_path);

        // Create connection pool with create_if_missing
        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await
            .context("Failed to connect to database")?;

        let db = Self { pool, db_path };
        
        // Run migrations
        db.migrate().await?;
        
        Ok(db)
    }

    /// Run database migrations
    async fn migrate(&self) -> Result<()> {
        info!("Running database migrations");

        // Create games table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS games (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                emulator TEXT NOT NULL,
                path TEXT,
                last_played DATETIME,
                total_saves INTEGER DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(name, emulator)
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        // Create saves table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS saves (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                game_id INTEGER NOT NULL,
                timestamp DATETIME NOT NULL,
                file_path TEXT NOT NULL,
                file_hash TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                backup_path TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        // Create settings table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for better performance
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_saves_game_id ON saves(game_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_saves_timestamp ON saves(timestamp)")
            .execute(&self.pool)
            .await?;

        debug!("Database migrations completed");
        Ok(())
    }

    /// Get or create a game entry
    pub async fn get_or_create_game(&self, name: &str, emulator: &str) -> Result<Game> {
        // Try to get existing game
        let existing = sqlx::query_as::<_, (i64, String, String, Option<String>, Option<DateTime<Utc>>, i32)>(
            "SELECT id, name, emulator, path, last_played, total_saves FROM games WHERE name = ? AND emulator = ?"
        )
        .bind(name)
        .bind(emulator)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id, name, emulator, path, last_played, total_saves)) = existing {
            return Ok(Game {
                id,
                name,
                emulator,
                path,
                last_played,
                total_saves,
            });
        }

        // Create new game
        let id = sqlx::query(
            "INSERT INTO games (name, emulator) VALUES (?, ?)"
        )
        .bind(name)
        .bind(emulator)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        info!("Created new game: {} ({})", name, emulator);

        Ok(Game {
            id,
            name: name.to_string(),
            emulator: emulator.to_string(),
            path: None,
            last_played: None,
            total_saves: 0,
        })
    }

    /// Record a new save
    pub async fn record_save(
        &self,
        game_id: i64,
        file_path: &str,
        file_hash: &str,
        file_size: i64,
        backup_path: Option<&str>,
    ) -> Result<Save> {
        // Get the next version number for this game
        let version: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM saves WHERE game_id = ?"
        )
        .bind(game_id)
        .fetch_one(&self.pool)
        .await?;

        let timestamp = Utc::now();

        let id = sqlx::query(
            r#"
            INSERT INTO saves (game_id, timestamp, file_path, file_hash, file_size, version, backup_path)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(game_id)
        .bind(&timestamp)
        .bind(file_path)
        .bind(file_hash)
        .bind(file_size)
        .bind(version)
        .bind(backup_path)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        // Update game's last_played and total_saves
        sqlx::query(
            "UPDATE games SET last_played = ?, total_saves = total_saves + 1 WHERE id = ?"
        )
        .bind(&timestamp)
        .bind(game_id)
        .execute(&self.pool)
        .await?;

        debug!("Recorded save #{} for game {}", version, game_id);

        Ok(Save {
            id,
            game_id,
            timestamp,
            file_path: file_path.to_string(),
            file_hash: file_hash.to_string(),
            file_size,
            version,
            backup_path: backup_path.map(|s| s.to_string()),
        })
    }

    /// Get saves for a game
    pub async fn get_saves_for_game(&self, game_id: i64, limit: Option<i32>) -> Result<Vec<Save>> {
        let query = if let Some(limit) = limit {
            format!(
                "SELECT id, game_id, timestamp, file_path, file_hash, file_size, version, backup_path 
                 FROM saves WHERE game_id = ? ORDER BY timestamp DESC LIMIT {}",
                limit
            )
        } else {
            "SELECT id, game_id, timestamp, file_path, file_hash, file_size, version, backup_path 
             FROM saves WHERE game_id = ? ORDER BY timestamp DESC".to_string()
        };

        let saves = sqlx::query(&query)
            .bind(game_id)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|row| Save {
                id: row.get(0),
                game_id: row.get(1),
                timestamp: row.get(2),
                file_path: row.get(3),
                file_hash: row.get(4),
                file_size: row.get(5),
                version: row.get(6),
                backup_path: row.get(7),
            })
            .collect();

        Ok(saves)
    }

    /// Clean up old saves, keeping only the last N saves for a game
    pub async fn cleanup_old_saves(&self, game_id: i64, keep_count: i32) -> Result<Vec<Save>> {
        // Get saves to delete (older than keep_count)
        let saves_to_delete = sqlx::query(&format!(
            "SELECT id, game_id, timestamp, file_path, file_hash, file_size, version, backup_path 
             FROM saves WHERE game_id = ? 
             ORDER BY timestamp DESC 
             LIMIT -1 OFFSET {}",
            keep_count
        ))
        .bind(game_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| Save {
            id: row.get(0),
            game_id: row.get(1),
            timestamp: row.get(2),
            file_path: row.get(3),
            file_hash: row.get(4),
            file_size: row.get(5),
            version: row.get(6),
            backup_path: row.get(7),
        })
        .collect::<Vec<_>>();

        if !saves_to_delete.is_empty() {
            // Delete from database
            let ids: Vec<i64> = saves_to_delete.iter().map(|s| s.id).collect();
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let query = format!("DELETE FROM saves WHERE id IN ({})", placeholders);
            
            let mut q = sqlx::query(&query);
            for id in &ids {
                q = q.bind(id);
            }
            q.execute(&self.pool).await?;

            info!("Cleaned up {} old saves for game {}", saves_to_delete.len(), game_id);
        }

        Ok(saves_to_delete)
    }

    /// Get all games
    pub async fn get_all_games(&self) -> Result<Vec<Game>> {
        let games = sqlx::query(
            "SELECT id, name, emulator, path, last_played, total_saves FROM games ORDER BY last_played DESC"
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| Game {
            id: row.get(0),
            name: row.get(1),
            emulator: row.get(2),
            path: row.get(3),
            last_played: row.get(4),
            total_saves: row.get(5),
        })
        .collect();

        Ok(games)
    }

    /// Get a setting value
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let value = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = ?"
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;

        Ok(value)
    }

    /// Set a setting value
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = ?, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(key)
        .bind(value)
        .bind(value)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get database statistics
    pub async fn get_stats(&self) -> Result<(i32, i32)> {
        let total_games: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM games")
            .fetch_one(&self.pool)
            .await?;

        let total_saves: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM saves")
            .fetch_one(&self.pool)
            .await?;

        Ok((total_games, total_saves))
    }
}