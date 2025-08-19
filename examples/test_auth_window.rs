use anyhow::Result;
use retrosave::ui::{AuthWindow, AuthWindowEvent};
use retrosave::sync::AuthManager;
use std::sync::Arc;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("retrosave=debug")
        .init();

    println!("Starting auth window test...");
    
    // Create auth manager
    let auth_manager = Arc::new(AuthManager::new("http://localhost:3000".to_string()));
    
    // Create auth window
    let (auth_window, mut event_receiver) = AuthWindow::new(auth_manager)?;
    
    // Show the window
    auth_window.show().await?;
    
    // Wait for auth events
    println!("Waiting for authentication...");
    
    while let Some(event) = event_receiver.recv().await {
        match event {
            AuthWindowEvent::Success { email } => {
                println!("✅ Authentication successful! User: {}", email);
                break;
            }
            AuthWindowEvent::Error(err) => {
                println!("❌ Authentication error: {}", err);
                // Continue waiting for retry
            }
            AuthWindowEvent::Cancelled => {
                println!("Authentication cancelled by user");
                break;
            }
        }
    }
    
    println!("Test completed");
    Ok(())
}