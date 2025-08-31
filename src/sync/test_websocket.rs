#[cfg(test)]
mod websocket_storage_tests {
    use super::super::*;
    use crate::sync::{AuthManager, SyncService, SyncEvent};
    use crate::sync::auth::AuthStateChanged;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use log;
    
    #[tokio::test]
    async fn test_websocket_storage() {
        // Initialize logging for tests
        let _ = env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .filter_module("retrosave", log::LevelFilter::Trace)
            .is_test(true)
            .try_init();
        
        println!("\n=== Starting WebSocket storage test ===");
        
        // Create channels
        let (sync_tx, _sync_rx) = mpsc::channel::<SyncEvent>(100);
        let (auth_tx, _auth_rx) = mpsc::channel::<AuthStateChanged>(100);
        
        // Create auth manager
        let auth_manager = Arc::new(AuthManager::new(auth_tx));
        
        // Create sync service with test API URL
        std::env::set_var("API_URL", "http://localhost:3001"); // Use a test URL
        
        let sync_service = Arc::new(
            SyncService::new(
                "test_device".to_string(),
                auth_manager.clone(),
                sync_tx.clone(),
            )
            .await
            .expect("Failed to create sync service")
        );
        
        println!("Created sync service");
        
        // Test 1: Check initial state
        println!("\nTest 1: Check initial WebSocket state");
        let handler = sync_service.get_event_handler().await;
        assert!(handler.is_none(), "WebSocket should initially be None");
        println!("✓ Initial state is None as expected");
        
        // Test 2: Try to initialize WebSocket with a test token
        println!("\nTest 2: Initialize WebSocket");
        let test_token = "test_token_12345".to_string();
        let service_arc = sync_service.clone();
        
        // This will likely fail to connect but we want to see the logs
        match service_arc.init_websocket_test(test_token).await {
            Ok(_) => println!("init_websocket returned Ok (unexpected)"),
            Err(e) => println!("init_websocket returned Err (expected): {}", e),
        }
        
        // Wait a bit for async operations
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        // Test 3: Check if WebSocket was stored (should still be None if connection failed)
        println!("\nTest 3: Check WebSocket after init attempt");
        let handler = sync_service.get_event_handler().await;
        if handler.is_some() {
            println!("✗ WebSocket was stored despite connection failure!");
        } else {
            println!("✓ WebSocket is still None after failed connection");
        }
        
        println!("\n=== Test complete ===");
    }
    
    #[tokio::test]
    async fn test_websocket_arc_handling() {
        let _ = env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .filter_module("retrosave", log::LevelFilter::Trace)
            .is_test(true)
            .try_init();
        
        println!("\n=== Testing Arc handling ===");
        
        // Test if the Arc<RwLock<Option<Arc<WebSocketClient>>>> works correctly
        use tokio::sync::RwLock;
        use super::super::websocket::WebSocketClient;
        
        let websocket: Arc<RwLock<Option<Arc<WebSocketClient>>>> = Arc::new(RwLock::new(None));
        
        // Create a mock client
        let (tx, _rx) = mpsc::unbounded_channel();
        let client = WebSocketClient::new("ws://test".to_string(), tx);
        let client_arc = Arc::new(client);
        
        println!("Created client Arc, strong_count: {}", Arc::strong_count(&client_arc));
        
        // Store it
        {
            let mut ws = websocket.write().await;
            *ws = Some(client_arc.clone());
            println!("Stored client, strong_count: {}", Arc::strong_count(&client_arc));
        }
        
        // Verify it's stored
        {
            let ws = websocket.read().await;
            assert!(ws.is_some(), "Client should be stored");
            if let Some(stored) = ws.as_ref() {
                println!("Retrieved client, strong_count: {}", Arc::strong_count(stored));
            }
        }
        
        println!("✓ Arc storage works correctly");
        println!("\n=== Arc test complete ===");
    }
}