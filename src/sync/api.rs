use anyhow::{Result, Context};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: Uuid,
    pub name: String,
    pub emulator: String,
    pub save_count: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveMetadata {
    pub id: Uuid,
    pub game_id: Uuid,
    pub file_hash: String,
    pub file_size: i64,
    pub client_timestamp: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub download_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RegisterGameRequest {
    pub name: String,
    pub emulator: String,
}

#[derive(Debug, Serialize)]
pub struct UploadSaveRequest {
    pub game_id: Uuid,
    pub file_hash: String,
    pub file_size: i64,
    pub client_timestamp: DateTime<Utc>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct UploadUrlResponse {
    pub save_id: Uuid,
    pub upload_url: String,
    pub expires_in: i64,
}

#[derive(Debug, Deserialize)]
pub struct ListSavesResponse {
    pub saves: Vec<SaveMetadata>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

pub struct SyncApi {
    client: Client,
    base_url: String,
    auth_manager: Arc<super::AuthManager>,
}

impl SyncApi {
    pub fn new(base_url: String, auth_manager: Arc<super::AuthManager>) -> Self {
        Self {
            client: Client::new(),
            base_url,
            auth_manager,
        }
    }

    /// Register a new game or get existing one
    pub async fn register_game(&self, name: &str, emulator: &str) -> Result<Game> {
        let token = self.auth_manager.get_access_token().await
            .context("Not authenticated")?;

        let response = self.client
            .post(format!("{}/api/saves/games/register", self.base_url))
            .bearer_auth(token)
            .json(&RegisterGameRequest {
                name: name.to_string(),
                emulator: emulator.to_string(),
            })
            .send()
            .await
            .context("Failed to register game")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Failed to register game: {}", error_text));
        }

        response.json().await
            .context("Failed to parse game response")
    }

    /// List all games for the current user
    pub async fn list_games(&self) -> Result<Vec<Game>> {
        let token = self.auth_manager.get_access_token().await
            .context("Not authenticated")?;

        let response = self.client
            .get(format!("{}/api/saves/games", self.base_url))
            .bearer_auth(token)
            .send()
            .await
            .context("Failed to list games")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to list games"));
        }

        response.json().await
            .context("Failed to parse games response")
    }

    /// Request upload URL for a save file
    pub async fn request_upload_url(
        &self,
        game_id: Uuid,
        file_hash: &str,
        file_size: i64,
        timestamp: DateTime<Utc>,
    ) -> Result<UploadUrlResponse> {
        let token = self.auth_manager.get_access_token().await
            .context("Not authenticated")?;

        let response = self.client
            .post(format!("{}/api/saves/upload", self.base_url))
            .bearer_auth(token)
            .json(&UploadSaveRequest {
                game_id,
                file_hash: file_hash.to_string(),
                file_size,
                client_timestamp: timestamp,
                metadata: None,
            })
            .send()
            .await
            .context("Failed to request upload URL")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Failed to request upload URL: {}", error_text));
        }

        response.json().await
            .context("Failed to parse upload response")
    }

    /// Upload save file data to presigned URL
    pub async fn upload_save_data(&self, upload_url: &str, data: Vec<u8>) -> Result<()> {
        let response = self.client
            .put(upload_url)
            .body(data)
            .send()
            .await
            .context("Failed to upload save data")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to upload save data to S3"));
        }

        Ok(())
    }

    /// List saves for the current user
    pub async fn list_saves(&self, game_id: Option<Uuid>, page: i64, per_page: i64) -> Result<ListSavesResponse> {
        let token = self.auth_manager.get_access_token().await
            .context("Not authenticated")?;

        let mut url = format!("{}/api/saves/list?page={}&per_page={}", self.base_url, page, per_page);
        
        if let Some(game_id) = game_id {
            url.push_str(&format!("&game_id={}", game_id));
        }

        let response = self.client
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .context("Failed to list saves")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to list saves"));
        }

        response.json().await
            .context("Failed to parse saves response")
    }

    /// Get a specific save with download URL
    pub async fn get_save(&self, save_id: Uuid) -> Result<SaveMetadata> {
        let token = self.auth_manager.get_access_token().await
            .context("Not authenticated")?;

        let response = self.client
            .get(format!("{}/api/saves/{}", self.base_url, save_id))
            .bearer_auth(token)
            .send()
            .await
            .context("Failed to get save")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get save"));
        }

        response.json().await
            .context("Failed to parse save response")
    }

    /// Download save file data from presigned URL
    pub async fn download_save_data(&self, download_url: &str) -> Result<Vec<u8>> {
        let response = self.client
            .get(download_url)
            .send()
            .await
            .context("Failed to download save data")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to download save data from S3"));
        }

        response.bytes().await
            .map(|b| b.to_vec())
            .context("Failed to read save data")
    }

    /// Delete a save
    pub async fn delete_save(&self, save_id: Uuid) -> Result<()> {
        let token = self.auth_manager.get_access_token().await
            .context("Not authenticated")?;

        let response = self.client
            .delete(format!("{}/api/saves/{}", self.base_url, save_id))
            .bearer_auth(token)
            .send()
            .await
            .context("Failed to delete save")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to delete save"));
        }

        Ok(())
    }
}