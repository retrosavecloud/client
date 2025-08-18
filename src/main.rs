use anyhow::Result;
use tracing::{info, error};
use tracing_subscriber;

mod monitor;
mod emulators;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("retrosave=debug")
        .init();

    info!("Starting Retrosave...");

    // Start process monitoring
    let monitor_handle = tokio::spawn(async move {
        if let Err(e) = monitor::start_monitoring().await {
            error!("Monitor error: {}", e);
        }
    });

    // For now, just wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("Shutting down Retrosave...");

    monitor_handle.abort();
    
    Ok(())
}