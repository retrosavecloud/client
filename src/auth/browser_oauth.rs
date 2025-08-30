use anyhow::{Result, Context};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::net::TcpListener;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, debug};

/// OAuth client for browser-based authentication
pub struct BrowserOAuth {
    api_base_url: String,
    #[allow(dead_code)]
    redirect_port: Option<u16>,
    state: Arc<RwLock<Option<OAuthState>>>,
}

/// OAuth state during flow
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct OAuthState {
    state: String,
    code_verifier: String,
    code_challenge: String,
}

/// Response from initiate endpoint
#[derive(Debug, Deserialize)]
struct InitiateResponse {
    auth_url: String,
    state: String,
}

/// Response from exchange endpoint
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub user: UserInfo,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub username: String,
}

impl BrowserOAuth {
    /// Create new OAuth client
    pub fn new(api_base_url: String) -> Self {
        Self {
            api_base_url,
            redirect_port: None,
            state: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Start OAuth flow
    pub async fn authenticate(&self) -> Result<TokenResponse> {
        // Find available port for callback
        let listener = TcpListener::bind("127.0.0.1:0")
            .context("Failed to bind to local port")?;
        let redirect_port = listener.local_addr()?.port();
        
        info!("Starting OAuth flow with callback on port {}", redirect_port);
        
        // Generate PKCE challenge
        let (code_verifier, code_challenge) = generate_pkce_pair();
        
        // Collect device information
        let device_info = collect_device_info();
        
        // Initiate OAuth flow with backend
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/api/auth/desktop/initiate", self.api_base_url))
            .json(&serde_json::json!({
                "code_challenge": code_challenge,
                "code_challenge_method": "S256",
                "redirect_port": redirect_port,
                "device_info": device_info,
            }))
            .send()
            .await
            .context("Failed to initiate OAuth flow")?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Failed to initiate OAuth: {}", error_text));
        }
        
        let initiate_response: InitiateResponse = response
            .json()
            .await
            .context("Failed to parse initiate response")?;
        
        // Store OAuth state
        {
            let mut state_lock = self.state.write().await;
            *state_lock = Some(OAuthState {
                state: initiate_response.state.clone(),
                code_verifier: code_verifier.clone(),
                code_challenge: code_challenge.clone(),
            });
        }
        
        // Open browser
        info!("Opening browser to: {}", initiate_response.auth_url);
        open::that(&initiate_response.auth_url)
            .context("Failed to open browser")?;
        
        // Wait for callback
        let (code, callback_state) = self.wait_for_callback(listener)
            .await
            .context("Failed to receive OAuth callback")?;
        
        // Verify state matches
        {
            let state_lock = self.state.read().await;
            if let Some(oauth_state) = &*state_lock {
                if oauth_state.state != callback_state {
                    return Err(anyhow::anyhow!("State mismatch in OAuth callback"));
                }
            } else {
                return Err(anyhow::anyhow!("No OAuth state found"));
            }
        }
        
        // Exchange code for tokens
        let tokens = self.exchange_code_for_tokens(code, callback_state, code_verifier)
            .await
            .context("Failed to exchange code for tokens")?;
        
        info!("OAuth flow completed successfully");
        Ok(tokens)
    }
    
    /// Wait for OAuth callback on local server with timeout
    async fn wait_for_callback(&self, listener: TcpListener) -> Result<(String, String)> {
        use std::io::{BufRead, BufReader};
        use std::time::Duration;
        use tokio::time::timeout;
        
        // Set non-blocking mode for async operation
        listener.set_nonblocking(true)?;
        
        // Create async listener
        let async_listener = tokio::net::TcpListener::from_std(listener)?;
        
        // Wait for connection with 5 minute timeout
        let accept_future = async {
            match async_listener.accept().await {
                Ok((stream, addr)) => {
                    debug!("Received callback connection from: {}", addr);
                    Ok(stream)
                }
                Err(e) => Err(anyhow::anyhow!("Failed to accept connection: {}", e))
            }
        };
        
        let stream = match timeout(Duration::from_secs(300), accept_future).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(anyhow::anyhow!("OAuth callback timeout after 5 minutes")),
        };
        
        // Convert back to std stream for reading
        let std_stream = stream.into_std()?;
        std_stream.set_nonblocking(false)?;
        
        // Read HTTP request
        let buf_reader = BufReader::new(&std_stream);
        let http_request: Vec<String> = buf_reader
            .lines()
            .map(|result| result.unwrap_or_default())
            .take_while(|line| !line.is_empty())
            .collect();
        
        // Parse GET request line
        let request_line = http_request.first()
            .ok_or_else(|| anyhow::anyhow!("Empty HTTP request"))?;
        
        // Extract path and query params
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(anyhow::anyhow!("Invalid HTTP request line"));
        }
        
        let path_and_query = parts[1];
        let url = format!("http://localhost{}", path_and_query);
        let parsed_url = url::Url::parse(&url)?;
        
        // Extract code and state from query params
        let mut code = None;
        let mut state = None;
        
        for (key, value) in parsed_url.query_pairs() {
            match key.as_ref() {
                "code" => code = Some(value.to_string()),
                "state" => state = Some(value.to_string()),
                _ => {}
            }
        }
        
        let code = code.ok_or_else(|| anyhow::anyhow!("No code in callback"))?;
        let state = state.ok_or_else(|| anyhow::anyhow!("No state in callback"))?;
        
        // Send minimal response - the backend already showed the styled success page
        // This just acknowledges receipt and closes immediately
        let response = r#"HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8

<!DOCTYPE html>
<html>
<head>
    <title>Processing...</title>
    <style>
        @import url('https://fonts.googleapis.com/css2?family=Orbitron:wght@400;700;900&display=swap');
        
        body {
            font-family: 'Orbitron', monospace;
            display: flex;
            align-items: center;
            justify-content: center;
            height: 100vh;
            margin: 0;
            background: #0a0e0a;
        }
        .fallback-message {
            text-align: center;
            padding: 3rem;
            background: rgba(17, 24, 39, 0.95);
            border: 2px solid oklch(0.65 0.25 142);
            border-radius: 16px;
            box-shadow: 0 0 40px oklch(0.65 0.25 142 / 0.3), 0 20px 60px rgba(0, 0, 0, 0.5);
            backdrop-filter: blur(10px);
            max-width: 500px;
        }
        .fallback-message h2 {
            color: oklch(0.65 0.25 142);
            font-size: 1.5rem;
            margin-bottom: 1rem;
            text-transform: uppercase;
            letter-spacing: 2px;
            text-shadow: 0 0 20px oklch(0.65 0.25 142 / 0.5);
        }
        .fallback-message p {
            color: #9ca3af;
            font-size: 1rem;
            line-height: 1.6;
            letter-spacing: 1px;
        }
        .success-badge {
            display: inline-block;
            background: oklch(0.65 0.25 142);
            color: white;
            padding: 0.5rem 1.5rem;
            border-radius: 50px;
            font-weight: 700;
            margin-bottom: 1.5rem;
            box-shadow: 0 0 20px oklch(0.65 0.25 142 / 0.4);
            animation: pulse 2s infinite;
        }
        @keyframes pulse {
            0% { transform: scale(1); box-shadow: 0 0 20px oklch(0.65 0.25 142 / 0.4); }
            50% { transform: scale(1.05); box-shadow: 0 0 30px oklch(0.65 0.25 142 / 0.6); }
            100% { transform: scale(1); box-shadow: 0 0 20px oklch(0.65 0.25 142 / 0.4); }
        }
    </style>
    <script>
        // Try to close immediately - the backend already handled the UI
        window.close();
        // Styled fallback message if close doesn't work
        setTimeout(() => {
            if (document.body) {
                document.body.innerHTML = `
                    <div class="fallback-message">
                        <div class="success-badge">✓ AUTHENTICATED</div>
                        <h2>Authentication Complete</h2>
                        <p>You have been successfully authenticated.</p>
                        <p>You can now close this window and return to RetroSave.</p>
                    </div>
                `;
            }
        }, 100);
    </script>
</head>
<body></body>
</html>"#;
        
        use std::io::Write;
        let mut std_stream_write = std_stream;
        std_stream_write.write_all(response.as_bytes())?;
        std_stream_write.flush()?;
        
        Ok((code, state))
    }
    
    /// Exchange authorization code for tokens
    async fn exchange_code_for_tokens(
        &self,
        code: String,
        state: String,
        code_verifier: String,
    ) -> Result<TokenResponse> {
        let client = reqwest::Client::new();
        
        let response = client
            .post(format!("{}/api/auth/desktop/exchange", self.api_base_url))
            .json(&serde_json::json!({
                "code": code,
                "state": state,
                "code_verifier": code_verifier,
            }))
            .send()
            .await
            .context("Failed to exchange code for tokens")?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Failed to exchange code: {}", error_text));
        }
        
        let tokens = response
            .json::<TokenResponse>()
            .await
            .context("Failed to parse token response")?;
        
        Ok(tokens)
    }
}

/// Generate a stable device fingerprint based on hardware/system characteristics
fn generate_device_fingerprint(hostname: &str, os_name: &str) -> String {
    use sha2::{Sha256, Digest};
    
    // Combine stable system properties to create a unique fingerprint
    // Using hostname + OS name + a stable machine identifier
    let mut components = vec![
        hostname.to_string(),
        os_name.to_string(),
    ];
    
    // Try to get machine ID on Linux
    #[cfg(target_os = "linux")]
    {
        if let Ok(machine_id) = std::fs::read_to_string("/etc/machine-id") {
            components.push(machine_id.trim().to_string());
        } else if let Ok(machine_id) = std::fs::read_to_string("/var/lib/dbus/machine-id") {
            components.push(machine_id.trim().to_string());
        }
    }
    
    // On Windows, use the ComputerName and other identifiers
    #[cfg(target_os = "windows")]
    {
        if let Ok(computer_name) = std::env::var("COMPUTERNAME") {
            components.push(computer_name);
        }
    }
    
    // On macOS, use the hardware UUID
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("ioreg")
            .args(&["-rd1", "-c", "IOPlatformExpertDevice"])
            .output() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            if let Some(start) = output_str.find("IOPlatformUUID") {
                if let Some(uuid_line) = output_str[start..].lines().next() {
                    if let Some(uuid) = uuid_line.split('"').nth(3) {
                        components.push(uuid.to_string());
                    }
                }
            }
        }
    }
    
    // Create hash from components
    let mut hasher = Sha256::new();
    hasher.update(components.join("-"));
    let result = hasher.finalize();
    
    // Return as hex string prefixed with "desktop-"
    format!("desktop-{}", hex::encode(&result[..16])) // Use first 16 bytes for shorter fingerprint
}

/// Collect device information for tracking
fn collect_device_info() -> serde_json::Value {
    use sysinfo::System;
    
    let mut sys = System::new();
    sys.refresh_all();
    
    // Get OS information
    let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
    let os_version = System::os_version().unwrap_or_else(|| "".to_string());
    let os_info = if os_version.is_empty() {
        os_name.clone()
    } else {
        format!("{} {}", os_name, os_version)
    };
    
    // Get device name (hostname)
    let device_name = System::host_name()
        .unwrap_or_else(|| "Desktop Client".to_string());
    
    // Get app version from cargo
    let app_version = env!("CARGO_PKG_VERSION");
    
    // Create a stable device fingerprint based on hostname and OS
    // This ensures the same device always has the same fingerprint
    let device_fingerprint = generate_device_fingerprint(&device_name, &os_name);
    
    serde_json::json!({
        "name": device_name,
        "os": os_info,
        "app_version": app_version,
        "device_fingerprint": device_fingerprint,
    })
}

/// Generate PKCE code verifier and challenge
fn generate_pkce_pair() -> (String, String) {
    // Generate random code verifier (43-128 characters)
    let mut verifier_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut verifier_bytes);
    let code_verifier = URL_SAFE_NO_PAD.encode(&verifier_bytes);
    
    // Generate code challenge (SHA256 of verifier)
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let result = hasher.finalize();
    let code_challenge = URL_SAFE_NO_PAD.encode(result);
    
    (code_verifier, code_challenge)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pkce_generation() {
        let (verifier, challenge) = generate_pkce_pair();
        
        // Verify lengths
        assert!(verifier.len() >= 43);
        assert!(challenge.len() >= 43);
        
        // Verify they're different
        assert_ne!(verifier, challenge);
        
        // Verify challenge is SHA256 of verifier
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let result = hasher.finalize();
        let expected_challenge = URL_SAFE_NO_PAD.encode(result);
        
        assert_eq!(challenge, expected_challenge);
    }
}