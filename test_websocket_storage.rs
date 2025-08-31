use retrosave::sync::SyncService;
use retrosave::auth::AuthManager;
use retrosave::sync::SyncEvent;
use retrosave::auth::AuthStateChanged;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    // Initialize logging
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .filter_module("retrosave", log::LevelFilter::Trace)
        .init();

    println!("Starting WebSocket storage test...");

    // Create channels
    let (sync_tx, mut sync_rx) = mpsc::channel::<SyncEvent>(100);
    let (auth_tx, mut _auth_rx) = mpsc::channel::<AuthStateChanged>(100);
    
    // Create auth manager
    let auth_manager = Arc::new(AuthManager::new(auth_tx));
    
    // Create sync service
    let sync_service = Arc::new(SyncService::new(
        "test_device".to_string(),
        auth_manager.clone(),
        sync_tx.clone(),
    ).await.expect("Failed to create sync service"));
    
    // Test manual WebSocket initialization
    println!("\nTest 1: Manual WebSocket initialization");
    let test_token = "test_token_12345".to_string();
    
    println!("Calling init_websocket directly...");
    let service_arc = sync_service.clone();
    
    // Try to initialize WebSocket
    match service_arc.clone().init_websocket_test(test_token).await {
        Ok(_) => println!("init_websocket returned Ok"),
        Err(e) => println!("init_websocket returned Err: {}", e),
    }
    
    // Wait a bit for async operations
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Check if WebSocket was stored
    println!("\nChecking if WebSocket was stored...");
    if let Some(handler) = sync_service.get_event_handler().await {
        println!("SUCCESS: Event handler retrieved!");
    } else {
        println!("FAILURE: Event handler is None");
    }
    
    // Also test wait_for_websocket
    println!("\nTest 2: Using wait_for_websocket");
    if let Some(handler) = sync_service.wait_for_websocket(2).await {
        println!("SUCCESS: wait_for_websocket returned handler!");
    } else {
        println!("FAILURE: wait_for_websocket returned None");
    }
    
    println!("\nTest complete");
}