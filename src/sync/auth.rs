use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{info, debug};
use base64::{Engine as _, engine::general_purpose};
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use argon2::Argon2;
use rand::RngCore;

// File-based storage for cross-platform compatibility
const AUTH_FILE_NAME: &str = "auth.json";

/// Storage format for tokens
#[derive(Debug, Serialize, Deserialize)]
struct TokenStorage {
    encrypted_data: String,  // Base64 encoded encrypted tokens
    nonce: String,          // Base64 encoded nonce for decryption
    salt: String,           // Base64 encoded salt for key derivation
    stored_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct AuthState {
    pub is_authenticated: bool,
    pub user: Option<UserInfo>,
    pub tokens: Option<AuthTokens>,
}

pub struct AuthManager {
    state: Arc<RwLock<AuthState>>,
    api_base_url: String,
}

impl AuthManager {
    pub fn new(api_base_url: String) -> Self {
        Self {
            state: Arc::new(RwLock::new(AuthState {
                is_authenticated: false,
                user: None,
                tokens: None,
            })),
            api_base_url,
        }
    }

    /// Initialize by loading tokens from keyring
    pub async fn init(&self) -> Result<()> {
        // Check for stored tokens
        if let Some(tokens) = self.load_tokens_from_keyring()? {
            // Validate stored tokens
            
            // Try to get user info with stored token
            if let Ok(user) = self.fetch_user_info(&tokens.access_token).await {
                let mut state = self.state.write().await;
                state.is_authenticated = true;
                state.user = Some(user.clone());
                state.tokens = Some(tokens);
                // Successfully authenticated with stored tokens
            } else {
                // Token expired, attempting refresh
                // Token might be expired, try to refresh
                if let Ok(new_tokens) = self.refresh_token(&tokens.refresh_token).await {
                    self.store_tokens(&new_tokens)?;
                    // Tokens refreshed successfully
                    // Re-fetch user info with new token
                    match self.fetch_user_info(&new_tokens.access_token).await {
                        Ok(user) => {
                            let mut state = self.state.write().await;
                            state.is_authenticated = true;
                            state.user = Some(user.clone());
                            state.tokens = Some(new_tokens);
                            // Successfully authenticated after refresh
                        },
                        Err(_) => {
                            // Still update tokens and mark as authenticated since refresh worked
                            let mut state = self.state.write().await;
                            state.is_authenticated = true;
                            state.tokens = Some(new_tokens);
                        }
                    }
                } else {
                    // Refresh failed, clearing invalid tokens
                    // Clear invalid tokens
                    self.clear_tokens()?;
                }
            }
        } else {
            // No stored tokens found
        }
        Ok(())
    }

    /// Register a new user
    pub async fn register(&self, email: &str, username: &str, password: &str) -> Result<()> {
        let client = reqwest::Client::new();
        
        #[derive(Serialize)]
        struct RegisterRequest<'a> {
            email: &'a str,
            username: &'a str,
            password: &'a str,
        }

        #[derive(Deserialize)]
        struct RegisterResponse {
            access_token: String,
            refresh_token: String,
            expires_in: i64,
            user: UserInfo,
        }

        let response = client
            .post(format!("{}/api/auth/register", self.api_base_url))
            .json(&RegisterRequest { email, username, password })
            .send()
            .await
            .context("Failed to send register request")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Registration failed: {}", error_text));
        }

        let register_response: RegisterResponse = response
            .json()
            .await
            .context("Failed to parse register response")?;

        let tokens = AuthTokens {
            access_token: register_response.access_token,
            refresh_token: register_response.refresh_token,
            expires_in: register_response.expires_in,
        };

        self.store_tokens(&tokens)?;

        let mut state = self.state.write().await;
        state.is_authenticated = true;
        state.user = Some(register_response.user);
        state.tokens = Some(tokens);

        // Successfully registered and authenticated
        Ok(())
    }

    /// Login with existing credentials
    pub async fn login(&self, email: &str, password: &str) -> Result<()> {
        let client = reqwest::Client::new();
        
        #[derive(Serialize)]
        struct LoginRequest<'a> {
            email: &'a str,
            password: &'a str,
        }

        #[derive(Deserialize)]
        struct LoginResponse {
            access_token: String,
            refresh_token: String,
            expires_in: i64,
            user: UserInfo,
        }

        let response = client
            .post(format!("{}/api/auth/login", self.api_base_url))
            .json(&LoginRequest { email, password })
            .send()
            .await
            .context("Failed to send login request")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Login failed: {}", error_text));
        }

        let login_response: LoginResponse = response
            .json()
            .await
            .context("Failed to parse login response")?;

        let tokens = AuthTokens {
            access_token: login_response.access_token,
            refresh_token: login_response.refresh_token,
            expires_in: login_response.expires_in,
        };

        self.store_tokens(&tokens)?;

        let mut state = self.state.write().await;
        state.is_authenticated = true;
        state.user = Some(login_response.user);
        state.tokens = Some(tokens);

        info!("Successfully logged in");
        Ok(())
    }

    /// Logout and clear tokens
    pub async fn logout(&self) -> Result<()> {
        let state = self.state.read().await;
        
        if let Some(tokens) = &state.tokens {
            let client = reqwest::Client::new();
            
            // Call logout endpoint to invalidate token on server
            let _ = client
                .post(format!("{}/api/auth/logout", self.api_base_url))
                .bearer_auth(&tokens.access_token)
                .send()
                .await;
        }
        
        drop(state); // Release read lock
        
        self.clear_tokens()?;
        
        let mut state = self.state.write().await;
        state.is_authenticated = false;
        state.user = None;
        state.tokens = None;

        info!("Logged out successfully");
        Ok(())
    }

    /// Check if authenticated
    pub async fn is_authenticated(&self) -> bool {
        let state = self.state.read().await;
        state.is_authenticated
    }
    
    /// Get current user info
    pub async fn get_user_info(&self) -> Option<UserInfo> {
        let state = self.state.read().await;
        state.user.clone()
    }
    
    /// Get current access token
    pub async fn get_access_token(&self) -> Option<String> {
        let state = self.state.read().await;
        state.tokens.as_ref().map(|t| t.access_token.clone())
    }
    
    /// Save tokens from OAuth flow
    pub async fn save_tokens(
        &self,
        access_token: String,
        refresh_token: String,
        user: crate::auth::UserInfo,
    ) -> Result<()> {
        // Convert user info to our internal format
        let user_info = UserInfo {
            id: user.id,
            email: user.email.clone(),
            username: user.username,
        };
        
        // Create tokens struct
        let tokens = AuthTokens {
            access_token,
            refresh_token,
            expires_in: 86400, // Default to 24 hours
        };
        
        // Store tokens securely
        self.store_tokens(&tokens)?;
        
        // Update state
        let mut state = self.state.write().await;
        state.is_authenticated = true;
        state.user = Some(user_info.clone());
        state.tokens = Some(tokens);
        
        info!("Successfully saved OAuth tokens for user: {}", user_info.email);
        Ok(())
    }

    /// Refresh the access token
    async fn refresh_token(&self, refresh_token: &str) -> Result<AuthTokens> {
        let client = reqwest::Client::new();
        
        #[derive(Serialize)]
        struct RefreshRequest<'a> {
            refresh_token: &'a str,
        }

        #[derive(Deserialize)]
        struct RefreshResponse {
            access_token: String,
            refresh_token: String,
            expires_in: i64,
        }

        let response = client
            .post(format!("{}/api/auth/refresh", self.api_base_url))
            .json(&RefreshRequest { refresh_token })
            .send()
            .await
            .context("Failed to send refresh request")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Token refresh failed"));
        }

        let refresh_response: RefreshResponse = response
            .json()
            .await
            .context("Failed to parse refresh response")?;

        Ok(AuthTokens {
            access_token: refresh_response.access_token,
            refresh_token: refresh_response.refresh_token,
            expires_in: refresh_response.expires_in,
        })
    }

    /// Get user info from API
    async fn fetch_user_info(&self, access_token: &str) -> Result<UserInfo> {
        let client = reqwest::Client::new();
        
        debug!("Fetching user profile with token: {}...", &access_token[..20.min(access_token.len())]);
        
        let response = client
            .get(format!("{}/api/auth/profile", self.api_base_url))
            .bearer_auth(access_token)
            .send()
            .await
            .context("Failed to get user info")?;

        if !response.status().is_success() {
            debug!("Profile fetch failed with status: {}", response.status());
            return Err(anyhow::anyhow!("Failed to get user info"));
        }

        response
            .json()
            .await
            .context("Failed to parse user info")
    }

    /// Get auth file path
    fn get_auth_file_path() -> Result<PathBuf> {
        let data_dir = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?
            .join("retrosave");
        
        // Create directory if it doesn't exist
        std::fs::create_dir_all(&data_dir)?;
        
        Ok(data_dir.join(AUTH_FILE_NAME))
    }
    
    /// Derive encryption key from machine-specific data
    fn derive_encryption_key(salt: &[u8]) -> Result<[u8; 32]> {
        // Use machine ID and username as key material
        let mut key_material = String::new();
        
        // Add username
        if let Ok(username) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
            key_material.push_str(&username);
        }
        
        // Add hostname
        key_material.push_str(&gethostname::gethostname().to_string_lossy());
        
        // Add a fixed application-specific string
        key_material.push_str("retrosave-auth-v1");
        
        // Derive key using Argon2
        let mut key = [0u8; 32];
        Argon2::default().hash_password_into(
            key_material.as_bytes(),
            salt,
            &mut key
        ).map_err(|e| anyhow::anyhow!("Failed to derive key: {}", e))?;
        
        Ok(key)
    }
    
    /// Store tokens in file (encrypted)
    fn store_tokens(&self, tokens: &AuthTokens) -> Result<()> {
        let auth_file = Self::get_auth_file_path()?;
        
        // Generate random salt and nonce
        let mut salt = [0u8; 32];
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut salt);
        OsRng.fill_bytes(&mut nonce_bytes);
        
        // Derive encryption key
        let key = Self::derive_encryption_key(&salt)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        // Serialize tokens to JSON
        let token_json = serde_json::to_string(&tokens)?;
        
        // Encrypt the token data
        let encrypted = cipher.encrypt(nonce, token_json.as_bytes())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
        
        // Create storage struct
        let storage = TokenStorage {
            encrypted_data: general_purpose::STANDARD.encode(&encrypted),
            nonce: general_purpose::STANDARD.encode(&nonce_bytes),
            salt: general_purpose::STANDARD.encode(&salt),
            stored_at: chrono::Utc::now().timestamp(),
        };
        
        // Serialize to JSON
        let json = serde_json::to_string_pretty(&storage)?;
        
        // Write to file with restricted permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&auth_file, json)?;
            let mut perms = std::fs::metadata(&auth_file)?.permissions();
            perms.set_mode(0o600); // Only user can read/write
            std::fs::set_permissions(&auth_file, perms)?;
        }
        
        #[cfg(not(unix))]
        {
            std::fs::write(&auth_file, json)?;
        }
        
        info!("Tokens stored to file successfully at {:?}", auth_file);
        Ok(())
    }

    /// Load tokens from file
    fn load_tokens_from_keyring(&self) -> Result<Option<AuthTokens>> {
        let auth_file = Self::get_auth_file_path()?;
        
        if !auth_file.exists() {
            debug!("No auth file found at {:?}", auth_file);
            return Ok(None);
        }
        
        // Read file
        let json = std::fs::read_to_string(&auth_file)
            .context("Failed to read auth file")?;
        
        // Parse JSON
        let storage: TokenStorage = serde_json::from_str(&json)
            .context("Failed to parse auth file")?;
        
        // Decode encrypted data, nonce, and salt
        let encrypted_data = general_purpose::STANDARD.decode(&storage.encrypted_data)
            .context("Failed to decode encrypted data")?;
        let nonce_bytes = general_purpose::STANDARD.decode(&storage.nonce)
            .context("Failed to decode nonce")?;
        let salt = general_purpose::STANDARD.decode(&storage.salt)
            .context("Failed to decode salt")?;
        
        // Derive decryption key using same salt
        let key = Self::derive_encryption_key(&salt)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        // Decrypt the data
        let decrypted = cipher.decrypt(nonce, encrypted_data.as_ref())
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;
        
        // Parse decrypted JSON to get tokens
        let tokens: AuthTokens = serde_json::from_slice(&decrypted)
            .context("Failed to parse decrypted tokens")?;
        
        info!("Found stored tokens in auth file");
        Ok(Some(tokens))
    }

    /// Clear tokens from file
    fn clear_tokens(&self) -> Result<()> {
        let auth_file = Self::get_auth_file_path()?;
        
        if auth_file.exists() {
            std::fs::remove_file(&auth_file)?;
            debug!("Auth file removed from {:?}", auth_file);
        }
        
        Ok(())
    }

    /// Get current auth state
    pub async fn get_state(&self) -> AuthState {
        self.state.read().await.clone()
    }
}