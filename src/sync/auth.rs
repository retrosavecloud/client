use anyhow::{Result, Context};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error, debug};

const SERVICE_NAME: &str = "retrosave";
const ACCESS_TOKEN_KEY: &str = "access_token";
const REFRESH_TOKEN_KEY: &str = "refresh_token";

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
        if let Some(tokens) = self.load_tokens_from_keyring()? {
            debug!("Found stored tokens, validating...");
            
            // Try to get user info with stored token
            if let Ok(user) = self.get_user_info(&tokens.access_token).await {
                let mut state = self.state.write().await;
                state.is_authenticated = true;
                state.user = Some(user);
                state.tokens = Some(tokens);
                info!("Authenticated from stored tokens");
            } else {
                // Token might be expired, try to refresh
                if let Ok(new_tokens) = self.refresh_token(&tokens.refresh_token).await {
                    self.store_tokens(&new_tokens)?;
                    info!("Refreshed expired tokens");
                } else {
                    // Clear invalid tokens
                    self.clear_tokens()?;
                }
            }
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

        info!("Successfully registered and authenticated");
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

    /// Get current access token
    pub async fn get_access_token(&self) -> Option<String> {
        let state = self.state.read().await;
        state.tokens.as_ref().map(|t| t.access_token.clone())
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
    async fn get_user_info(&self, access_token: &str) -> Result<UserInfo> {
        let client = reqwest::Client::new();
        
        let response = client
            .get(format!("{}/api/auth/me", self.api_base_url))
            .bearer_auth(access_token)
            .send()
            .await
            .context("Failed to get user info")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to get user info"));
        }

        response
            .json()
            .await
            .context("Failed to parse user info")
    }

    /// Store tokens in keyring
    fn store_tokens(&self, tokens: &AuthTokens) -> Result<()> {
        let access_entry = Entry::new(SERVICE_NAME, ACCESS_TOKEN_KEY)?;
        access_entry.set_password(&tokens.access_token)
            .context("Failed to store access token")?;

        let refresh_entry = Entry::new(SERVICE_NAME, REFRESH_TOKEN_KEY)?;
        refresh_entry.set_password(&tokens.refresh_token)
            .context("Failed to store refresh token")?;

        debug!("Tokens stored in keyring");
        Ok(())
    }

    /// Load tokens from keyring
    fn load_tokens_from_keyring(&self) -> Result<Option<AuthTokens>> {
        let access_entry = Entry::new(SERVICE_NAME, ACCESS_TOKEN_KEY)?;
        let refresh_entry = Entry::new(SERVICE_NAME, REFRESH_TOKEN_KEY)?;

        match (access_entry.get_password(), refresh_entry.get_password()) {
            (Ok(access_token), Ok(refresh_token)) => {
                Ok(Some(AuthTokens {
                    access_token,
                    refresh_token,
                    expires_in: 86400, // Default to 24 hours
                }))
            }
            _ => Ok(None),
        }
    }

    /// Clear tokens from keyring
    fn clear_tokens(&self) -> Result<()> {
        let access_entry = Entry::new(SERVICE_NAME, ACCESS_TOKEN_KEY)?;
        let refresh_entry = Entry::new(SERVICE_NAME, REFRESH_TOKEN_KEY)?;
        
        let _ = access_entry.delete_credential();
        let _ = refresh_entry.delete_credential();
        
        debug!("Tokens cleared from keyring");
        Ok(())
    }

    /// Get current auth state
    pub async fn get_state(&self) -> AuthState {
        self.state.read().await.clone()
    }
}