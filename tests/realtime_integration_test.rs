use retrosave::sync::{WebSocketClient, EventHandler, MessageThrottler, ThrottleConfig};
use retrosave::payment::{SubscriptionInfo, UsageInfo};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// Full integration test simulating real-world event flow
#[tokio::test]
async fn test_complete_event_pipeline() {
    // Setup components
    let event_handler = Arc::new(EventHandler::new());
    let throttler = Arc::new(MessageThrottler::new(ThrottleConfig {
        ui_update_interval: Duration::from_millis(100),
        dedup_window: Duration::from_millis(500),
        batch_size: 5,
        batch_timeout: Duration::from_millis(50),
    }));
    
    // Track received events
    let subscription_updates = Arc::new(Mutex::new(Vec::new()));
    let usage_updates = Arc::new(Mutex::new(Vec::new()));
    
    let sub_clone = subscription_updates.clone();
    let usage_clone = usage_updates.clone();
    
    // Register callbacks
    event_handler.on_subscription_update(Box::new(move |info| {
        sub_clone.lock().unwrap().push(info);
    })).await;
    
    event_handler.on_usage_update(Box::new(move |info| {
        usage_clone.lock().unwrap().push(info);
    })).await;
    
    // Simulate WebSocket connection (mock for testing)
    let ws_client = create_mock_websocket().await;
    
    // Connect event handler to WebSocket
    let handler_clone = event_handler.clone();
    let throttler_clone = throttler.clone();
    
    tokio::spawn(async move {
        while let Ok(msg) = ws_client.receive().await {
            // Process through throttler
            if let Some(processed) = throttler_clone.process_incoming(msg.clone()).await {
                // Only update UI if not throttled
                let msg_type = get_message_type(&processed);
                if throttler_clone.should_update_ui(&msg_type).await {
                    handler_clone.handle_websocket_message(
                        serde_json::from_value(processed).unwrap()
                    ).await;
                }
            }
        }
    });
    
    // Simulate subscription upgrade
    simulate_subscription_upgrade(&ws_client).await;
    
    // Simulate rapid usage updates
    for i in 0..10 {
        simulate_usage_update(&ws_client, i).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    
    // Wait for processing
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Verify results
    assert!(!subscription_updates.lock().unwrap().is_empty());
    
    // Usage updates should be throttled
    let usage_count = usage_updates.lock().unwrap().len();
    assert!(usage_count < 10, "Expected throttling, got {} updates", usage_count);
}

#[tokio::test]
async fn test_offline_to_online_transition() {
    // Simulate user going from offline to online
    let event_handler = Arc::new(EventHandler::new());
    let received_events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = received_events.clone();
    
    // Register catch-all callback
    event_handler.on_subscription_update(Box::new(move |info| {
        events_clone.lock().unwrap().push(format!("subscription: {}", info.tier.id));
    })).await;
    
    // Simulate queued events being delivered when user comes online
    let queued_events = vec![
        create_subscription_message("pro"),
        create_usage_message(100, 50),
        create_device_added_message("device1"),
    ];
    
    // Process queued events
    for event in queued_events {
        event_handler.handle_websocket_message(event).await;
    }
    
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Should have received all queued events
    assert!(!received_events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_rate_limit_recovery() {
    let throttler = Arc::new(MessageThrottler::new(ThrottleConfig {
        ui_update_interval: Duration::from_millis(10),
        ..Default::default()
    }));
    
    let mut processed_count = 0;
    
    // Simulate burst of messages
    for i in 0..100 {
        let msg = serde_json::json!({
            "type": "usage_update",
            "count": i
        });
        
        if throttler.process_incoming(msg).await.is_some() {
            if throttler.should_update_ui("usage_update").await {
                processed_count += 1;
            }
        }
        
        // Small delay between messages
        if i % 10 == 0 {
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }
    
    // Should have throttled most messages
    assert!(processed_count < 100);
    assert!(processed_count > 0);
}

#[tokio::test]
async fn test_concurrent_device_connections() {
    // Simulate multiple devices connected for same user
    let event_handler = Arc::new(EventHandler::new());
    let device_events = Arc::new(Mutex::new(Vec::new()));
    
    // Spawn multiple "device" connections
    let mut handles = Vec::new();
    for device_id in 0..5 {
        let handler = event_handler.clone();
        let events = device_events.clone();
        
        let handle = tokio::spawn(async move {
            // Register callback for this device
            let events_clone = events.clone();
            handler.on_device_added(Box::new(move |id, name, dtype| {
                events_clone.lock().unwrap().push((id, name, dtype));
            })).await;
            
            // Simulate device activity
            let msg = create_device_added_message(&format!("device_{}", device_id));
            handler.handle_websocket_message(msg).await;
        });
        
        handles.push(handle);
    }
    
    // Wait for all devices
    for handle in handles {
        handle.await.unwrap();
    }
    
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Should have events from all devices
    assert_eq!(device_events.lock().unwrap().len(), 5);
}

#[tokio::test]
async fn test_priority_event_ordering() {
    let throttler = Arc::new(MessageThrottler::new(ThrottleConfig::default()));
    let processor = retrosave::sync::PriorityProcessor::new(throttler);
    
    let mut results = Vec::new();
    
    // Process mix of priority levels
    let events = vec![
        ("save_uploaded", serde_json::json!({"save": "data"}), false),
        ("subscription_updated", serde_json::json!({"tier": "pro"}), true),
        ("usage_updated", serde_json::json!({"count": 10}), false),
        ("error", serde_json::json!({"error": "critical"}), true),
        ("device_added", serde_json::json!({"device": "new"}), false),
        ("storage_limit_warning", serde_json::json!({"percent": 90}), true),
    ];
    
    for (event_type, data, is_priority) in events {
        match processor.process(event_type, data).await {
            retrosave::sync::message_throttler::ProcessResult::Immediate(_) => {
                assert!(is_priority, "{} should not be immediate", event_type);
                results.push((event_type, "immediate"));
            }
            retrosave::sync::message_throttler::ProcessResult::Normal(_) => {
                assert!(!is_priority, "{} should be immediate", event_type);
                results.push((event_type, "normal"));
            }
            _ => {}
        }
    }
    
    // Verify priority events were processed immediately
    assert_eq!(results.iter().filter(|(_, p)| *p == "immediate").count(), 3);
}

#[tokio::test]
async fn test_websocket_reconnection_handling() {
    let event_handler = Arc::new(EventHandler::new());
    let connection_events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = connection_events.clone();
    
    // Track connection state changes
    event_handler.on_subscription_update(Box::new(move |_| {
        events_clone.lock().unwrap().push("connected");
    })).await;
    
    // Simulate connection
    let msg = create_subscription_message("pro");
    event_handler.handle_websocket_message(msg.clone()).await;
    
    // Simulate disconnect and reconnect with same data
    tokio::time::sleep(Duration::from_millis(100)).await;
    event_handler.handle_websocket_message(msg).await;
    
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Should have handled both connection events
    assert_eq!(connection_events.lock().unwrap().len(), 2);
}

// Helper functions

async fn create_mock_websocket() -> MockWebSocket {
    let (tx, rx) = mpsc::channel(100);
    MockWebSocket { tx, rx }
}

struct MockWebSocket {
    tx: mpsc::Sender<serde_json::Value>,
    rx: mpsc::Receiver<serde_json::Value>,
}

impl MockWebSocket {
    async fn receive(&mut self) -> Result<serde_json::Value, ()> {
        self.rx.recv().await.ok_or(())
    }
    
    async fn send(&self, msg: serde_json::Value) {
        let _ = self.tx.send(msg).await;
    }
}

async fn simulate_subscription_upgrade(ws: &MockWebSocket) {
    ws.send(serde_json::json!({
        "type": "subscription_updated",
        "tier": "pro",
        "status": "active",
        "billing_period": "monthly",
        "limits": {
            "saves": 10000,
            "storage_gb": 50,
            "devices": 5,
            "family_members": 0
        }
    })).await;
}

async fn simulate_usage_update(ws: &MockWebSocket, count: i32) {
    ws.send(serde_json::json!({
        "type": "usage_updated",
        "saves_count": count,
        "saves_limit": 50,
        "storage_bytes": count * 1000000,
        "storage_limit_bytes": 1000000000,
        "devices_count": 2,
        "devices_limit": 3
    })).await;
}

fn create_subscription_message(tier: &str) -> retrosave::sync::WsMessage {
    retrosave::sync::WsMessage::SubscriptionUpdated {
        tier: tier.to_string(),
        status: "active".to_string(),
        billing_period: "monthly".to_string(),
        limits: retrosave::sync::websocket::SubscriptionLimits {
            saves: Some(10000),
            storage_gb: 50,
            devices: 5,
            family_members: 0,
        },
    }
}

fn create_usage_message(saves: i32, storage_gb: i64) -> retrosave::sync::WsMessage {
    retrosave::sync::WsMessage::UsageUpdated {
        saves_count: saves,
        saves_limit: 1000,
        storage_bytes: storage_gb * 1024 * 1024 * 1024,
        storage_limit_bytes: 100 * 1024 * 1024 * 1024,
        devices_count: 2,
        devices_limit: 5,
    }
}

fn create_device_added_message(device_id: &str) -> retrosave::sync::WsMessage {
    retrosave::sync::WsMessage::DeviceAdded {
        device_id: device_id.to_string(),
        device_name: format!("Device {}", device_id),
        device_type: "desktop".to_string(),
    }
}

fn get_message_type(msg: &serde_json::Value) -> String {
    msg.get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("unknown")
        .to_string()
}