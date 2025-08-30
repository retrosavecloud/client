use anyhow::{Result, Context};
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce, Key
};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use argon2::password_hash::{PasswordHash, SaltString};
use base64::{Engine as _, engine::general_purpose};
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Encrypted save file metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedSave {
    /// Nonce used for encryption (base64)
    pub nonce: String,
    /// Salt used for key derivation (base64)
    pub salt: String,
    /// Encrypted data (base64)
    pub data: String,
    /// SHA256 hash of original data for integrity
    pub original_hash: String,
    /// Version of encryption scheme
    pub version: u8,
}

/// E2E Encryption manager for save files
pub struct EncryptionManager {
    /// User's encryption key derived from password
    master_key: Option<[u8; 32]>,
    /// Path to store encryption keys
    key_store_path: PathBuf,
}

impl EncryptionManager {
    /// Create a new encryption manager
    pub fn new(data_dir: Option<PathBuf>) -> Self {
        let key_store_path = data_dir
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".retrosave_keys");
        
        Self {
            master_key: None,
            key_store_path,
        }
    }
    
    /// Initialize with user's password
    pub async fn init_with_password(&mut self, password: &str) -> Result<()> {
        info!("Initializing E2E encryption");
        
        // Check if we have a stored key verification
        if self.key_store_path.exists() {
            // Verify password against stored verification
            let stored = tokio::fs::read_to_string(&self.key_store_path).await
                .context("Failed to read key store")?;
            
            let verification: KeyVerification = serde_json::from_str(&stored)
                .context("Failed to parse key verification")?;
            
            // Verify password
            if !self.verify_password(password, &verification)? {
                return Err(anyhow::anyhow!("Invalid encryption password"));
            }
            
            // Derive the same key
            let salt_bytes = general_purpose::STANDARD.decode(&verification.salt)?;
            self.master_key = Some(self.derive_key_from_password(
                password,
                &salt_bytes
            )?);
        } else {
            // First time setup - create new key
            let salt = SaltString::generate(&mut OsRng);
            let salt_bytes = salt.as_str().as_bytes();
            let key = self.derive_key_from_password(password, salt_bytes)?;
            
            // Store verification
            let verification = self.create_key_verification(password, salt.as_str())?;
            let json = serde_json::to_string_pretty(&verification)?;
            
            // Create directory if needed
            if let Some(parent) = self.key_store_path.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
            
            tokio::fs::write(&self.key_store_path, json).await
                .context("Failed to store key verification")?;
            
            self.master_key = Some(key);
            info!("E2E encryption initialized successfully");
        }
        
        Ok(())
    }
    
    /// Encrypt save data before upload
    pub fn encrypt_save(&self, data: &[u8]) -> Result<EncryptedSave> {
        let key = self.master_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Encryption not initialized"))?;
        
        debug!("Encrypting save data ({} bytes)", data.len());
        
        // Generate nonce and salt
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let salt = SaltString::generate(&mut OsRng);
        
        // Create cipher
        let cipher_key = Key::<Aes256Gcm>::from_slice(key);
        let cipher = Aes256Gcm::new(cipher_key);
        
        // Encrypt data
        let encrypted = cipher
            .encrypt(&nonce, data)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
        
        // Calculate hash of original data
        let mut hasher = Sha256::new();
        hasher.update(data);
        let original_hash = format!("{:x}", hasher.finalize());
        
        Ok(EncryptedSave {
            nonce: general_purpose::STANDARD.encode(&nonce),
            salt: salt.to_string(),
            data: general_purpose::STANDARD.encode(&encrypted),
            original_hash,
            version: 1,
        })
    }
    
    /// Decrypt save data after download
    pub fn decrypt_save(&self, encrypted: &EncryptedSave) -> Result<Vec<u8>> {
        let key = self.master_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Encryption not initialized"))?;
        
        debug!("Decrypting save data");
        
        // Decode from base64
        let nonce_bytes = general_purpose::STANDARD.decode(&encrypted.nonce)
            .context("Failed to decode nonce")?;
        let encrypted_data = general_purpose::STANDARD.decode(&encrypted.data)
            .context("Failed to decode encrypted data")?;
        
        // Create cipher
        let cipher_key = Key::<Aes256Gcm>::from_slice(key);
        let cipher = Aes256Gcm::new(cipher_key);
        
        // Decrypt data
        let nonce = Nonce::from_slice(&nonce_bytes);
        let decrypted = cipher
            .decrypt(nonce, encrypted_data.as_ref())
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;
        
        // Verify integrity
        let mut hasher = Sha256::new();
        hasher.update(&decrypted);
        let hash = format!("{:x}", hasher.finalize());
        
        if hash != encrypted.original_hash {
            return Err(anyhow::anyhow!("Data integrity check failed"));
        }
        
        Ok(decrypted)
    }
    
    /// Change encryption password
    pub async fn change_password(&mut self, old_password: &str, new_password: &str) -> Result<()> {
        // Verify old password
        if self.key_store_path.exists() {
            let stored = tokio::fs::read_to_string(&self.key_store_path).await?;
            let verification: KeyVerification = serde_json::from_str(&stored)?;
            
            if !self.verify_password(old_password, &verification)? {
                return Err(anyhow::anyhow!("Invalid current password"));
            }
        }
        
        // Generate new key
        let salt = SaltString::generate(&mut OsRng);
        let salt_bytes = salt.as_str().as_bytes();
        let key = self.derive_key_from_password(new_password, salt_bytes)?;
        
        // Store new verification
        let verification = self.create_key_verification(new_password, salt.as_str())?;
        let json = serde_json::to_string_pretty(&verification)?;
        tokio::fs::write(&self.key_store_path, json).await?;
        
        self.master_key = Some(key);
        info!("Encryption password changed successfully");
        
        Ok(())
    }
    
    /// Check if encryption is enabled
    pub fn is_enabled(&self) -> bool {
        self.master_key.is_some()
    }
    
    /// Derive encryption key from password
    fn derive_key_from_password(&self, password: &str, salt: &[u8]) -> Result<[u8; 32]> {
        let argon2 = Argon2::default();
        let mut key = [0u8; 32];
        
        argon2.hash_password_into(
            password.as_bytes(),
            salt,
            &mut key
        ).map_err(|e| anyhow::anyhow!("Key derivation failed: {}", e))?;
        
        Ok(key)
    }
    
    /// Create key verification data
    fn create_key_verification(&self, password: &str, salt: &str) -> Result<KeyVerification> {
        let argon2 = Argon2::default();
        let salt_string = SaltString::from_b64(salt)
            .map_err(|e| anyhow::anyhow!("Invalid salt: {}", e))?;
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt_string)
            .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?
            .to_string();
        
        Ok(KeyVerification {
            hash: password_hash,
            salt: general_purpose::STANDARD.encode(salt),
            version: 1,
        })
    }
    
    /// Verify password against stored verification
    fn verify_password(&self, password: &str, verification: &KeyVerification) -> Result<bool> {
        let parsed_hash = PasswordHash::new(&verification.hash)
            .map_err(|e| anyhow::anyhow!("Failed to parse password hash: {}", e))?;
        
        let argon2 = Argon2::default();
        Ok(argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok())
    }
    
    /// Export encryption key (for backup)
    pub fn export_key(&self) -> Result<String> {
        let key = self.master_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Encryption not initialized"))?;
        
        Ok(general_purpose::STANDARD.encode(key))
    }
    
    /// Import encryption key (from backup)
    pub fn import_key(&mut self, key_string: &str) -> Result<()> {
        let key_bytes = general_purpose::STANDARD.decode(key_string)
            .context("Invalid key format")?;
        
        if key_bytes.len() != 32 {
            return Err(anyhow::anyhow!("Invalid key length"));
        }
        
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);
        self.master_key = Some(key);
        
        Ok(())
    }
}

/// Key verification data stored on disk
#[derive(Debug, Serialize, Deserialize)]
struct KeyVerification {
    /// Argon2 password hash for verification
    hash: String,
    /// Salt used for hashing (base64)
    salt: String,
    /// Version of verification scheme
    version: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_encryption_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = EncryptionManager::new(Some(temp_dir.path().to_path_buf()));
        
        // Initialize with password
        manager.init_with_password("test_password_123").await.unwrap();
        
        // Test data
        let original_data = b"This is test save data for encryption";
        
        // Encrypt
        let encrypted = manager.encrypt_save(original_data).unwrap();
        assert!(!encrypted.data.is_empty());
        assert!(!encrypted.nonce.is_empty());
        
        // Decrypt
        let decrypted = manager.decrypt_save(&encrypted).unwrap();
        assert_eq!(decrypted, original_data);
    }
    
    #[tokio::test]
    async fn test_wrong_password() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager1 = EncryptionManager::new(Some(temp_dir.path().to_path_buf()));
        
        // Initialize with first password
        manager1.init_with_password("password1").await.unwrap();
        
        // Try to initialize with wrong password
        let mut manager2 = EncryptionManager::new(Some(temp_dir.path().to_path_buf()));
        let result = manager2.init_with_password("wrong_password").await;
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_password_change() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = EncryptionManager::new(Some(temp_dir.path().to_path_buf()));
        
        // Initialize with password
        manager.init_with_password("old_password").await.unwrap();
        
        // Encrypt data with old password
        let data = b"Test data";
        let encrypted = manager.encrypt_save(data).unwrap();
        
        // Change password
        manager.change_password("old_password", "new_password").await.unwrap();
        
        // Should still decrypt old data
        let decrypted = manager.decrypt_save(&encrypted).unwrap();
        assert_eq!(decrypted, data);
        
        // New encryption should work
        let encrypted2 = manager.encrypt_save(data).unwrap();
        let decrypted2 = manager.decrypt_save(&encrypted2).unwrap();
        assert_eq!(decrypted2, data);
    }
}