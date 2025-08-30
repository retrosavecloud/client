use retrosave::sync::{EventHandler, WsMessage};
use retrosave::payment::{SubscriptionInfo, UsageInfo, TierInfo};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_subscription_event_handling() {
    let event_handler = EventHandler::new();
    let received = Arc::new(Mutex::new(None));
    let received_clone = received.clone();
    
    // Register callback
    event_handler.on_subscription_update(Box::new(move |info| {
        let mut r = received_clone.lock().unwrap();
        *r = Some(info);
    })).await;
    
    // Create test message
    let msg = WsMessage::SubscriptionUpdated {
        tier: "pro".to_string(),
        status: "active".to_string(),
        billing_period: "monthly".to_string(),
        limits: retrosave::sync::websocket::SubscriptionLimits {
            saves: Some(10000),
            storage_gb: 50,
            devices: 5,
            family_members: 0,
        },
    };
    
    // Process message
    event_handler.handle_websocket_message(msg).await;
    
    // Give callback time to execute
    sleep(Duration::from_millis(10)).await;
    
    // Verify callback was called
    let received = received.lock().unwrap();
    assert!(received.is_some());
    let info = received.as_ref().unwrap();
    assert_eq!(info.tier.id, "pro");
    assert_eq!(info.status, "active");
}

#[tokio::test]
async fn test_usage_event_handling() {
    let event_handler = EventHandler::new();
    let received = Arc::new(Mutex::new(None));
    let received_clone = received.clone();
    
    // Register callback
    event_handler.on_usage_update(Box::new(move |info| {
        let mut r = received_clone.lock().unwrap();
        *r = Some(info);
    })).await;
    
    // Create test message
    let msg = WsMessage::UsageUpdated {
        saves_count: 42,
        saves_limit: 50,
        storage_bytes: 1024 * 1024 * 100, // 100MB
        storage_limit_bytes: 1024 * 1024 * 1024, // 1GB
        devices_count: 2,
        devices_limit: 3,
    };
    
    // Process message
    event_handler.handle_websocket_message(msg).await;
    
    // Give callback time to execute
    sleep(Duration::from_millis(10)).await;
    
    // Verify callback was called
    let received = received.lock().unwrap();
    assert!(received.is_some());
    let info = received.as_ref().unwrap();
    assert_eq!(info.saves_count, 42);
    assert_eq!(info.storage_bytes, 1024 * 1024 * 100);
}

#[tokio::test]
async fn test_device_events() {
    let event_handler = EventHandler::new();
    let added_devices = Arc::new(Mutex::new(Vec::new()));
    let removed_devices = Arc::new(Mutex::new(Vec::new()));
    
    let added_clone = added_devices.clone();
    let removed_clone = removed_devices.clone();
    
    // Register callbacks
    event_handler.on_device_added(Box::new(move |id, name, device_type| {
        added_clone.lock().unwrap().push((id, name, device_type));
    })).await;
    
    event_handler.on_device_removed(Box::new(move |id, name| {
        removed_clone.lock().unwrap().push((id, name));
    })).await;
    
    // Process add event
    let add_msg = WsMessage::DeviceAdded {
        device_id: "dev123".to_string(),
        device_name: "Test PC".to_string(),
        device_type: "desktop".to_string(),
    };
    event_handler.handle_websocket_message(add_msg).await;
    
    // Process remove event
    let remove_msg = WsMessage::DeviceRemoved {
        device_id: "dev123".to_string(),
        device_name: "Test PC".to_string(),
    };
    event_handler.handle_websocket_message(remove_msg).await;
    
    sleep(Duration::from_millis(10)).await;
    
    // Verify callbacks
    assert_eq!(added_devices.lock().unwrap().len(), 1);
    assert_eq!(removed_devices.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn test_warning_events() {
    let event_handler = EventHandler::new();
    let storage_warnings = Arc::new(Mutex::new(Vec::new()));
    let save_warnings = Arc::new(Mutex::new(Vec::new()));
    
    let storage_clone = storage_warnings.clone();
    let save_clone = save_warnings.clone();
    
    // Register callbacks
    event_handler.on_storage_warning(Box::new(move |percentage, message| {
        storage_clone.lock().unwrap().push((percentage, message));
    })).await;
    
    event_handler.on_save_limit_warning(Box::new(move |percentage, message| {
        save_clone.lock().unwrap().push((percentage, message));
    })).await;
    
    // Process warnings
    let storage_msg = WsMessage::StorageLimitWarning {
        percentage: 85.0,
        message: "Storage is 85% full".to_string(),
    };
    event_handler.handle_websocket_message(storage_msg).await;
    
    let save_msg = WsMessage::SaveLimitWarning {
        percentage: 90.0,
        message: "90% of save limit reached".to_string(),
    };
    event_handler.handle_websocket_message(save_msg).await;
    
    sleep(Duration::from_millis(10)).await;
    
    // Verify warnings received
    assert_eq!(storage_warnings.lock().unwrap().len(), 1);
    assert_eq!(save_warnings.lock().unwrap().len(), 1);
    
    let (percentage, _) = &storage_warnings.lock().unwrap()[0];
    assert_eq!(*percentage, 85.0);
}

#[tokio::test]
async fn test_multiple_callbacks() {
    let event_handler = EventHandler::new();
    let counter1 = Arc::new(Mutex::new(0));
    let counter2 = Arc::new(Mutex::new(0));
    
    let c1 = counter1.clone();
    let c2 = counter2.clone();
    
    // Register multiple callbacks for same event
    event_handler.on_subscription_update(Box::new(move |_| {
        *c1.lock().unwrap() += 1;
    })).await;
    
    event_handler.on_subscription_update(Box::new(move |_| {
        *c2.lock().unwrap() += 1;
    })).await;
    
    // Process event
    let msg = WsMessage::SubscriptionUpdated {
        tier: "pro".to_string(),
        status: "active".to_string(),
        billing_period: "monthly".to_string(),
        limits: retrosave::sync::websocket::SubscriptionLimits {
            saves: Some(10000),
            storage_gb: 50,
            devices: 5,
            family_members: 0,
        },
    };
    
    event_handler.handle_websocket_message(msg).await;
    sleep(Duration::from_millis(10)).await;
    
    // Both callbacks should be called
    assert_eq!(*counter1.lock().unwrap(), 1);
    assert_eq!(*counter2.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_event_handler_cleanup() {
    let event_handler = EventHandler::new();
    let counter = Arc::new(Mutex::new(0));
    let counter_clone = counter.clone();
    
    // Register callback
    event_handler.on_subscription_update(Box::new(move |_| {
        *counter_clone.lock().unwrap() += 1;
    })).await;
    
    // Clear callbacks
    event_handler.clear_all_callbacks().await;
    
    // Process event
    let msg = WsMessage::SubscriptionUpdated {
        tier: "pro".to_string(),
        status: "active".to_string(),
        billing_period: "monthly".to_string(),
        limits: retrosave::sync::websocket::SubscriptionLimits {
            saves: Some(10000),
            storage_gb: 50,
            devices: 5,
            family_members: 0,
        },
    };
    
    event_handler.handle_websocket_message(msg).await;
    sleep(Duration::from_millis(10)).await;
    
    // Callback should not be called after clearing
    assert_eq!(*counter.lock().unwrap(), 0);
}

#[tokio::test]
async fn test_concurrent_event_processing() {
    let event_handler = Arc::new(EventHandler::new());
    let counter = Arc::new(Mutex::new(0));
    let counter_clone = counter.clone();
    
    // Register callback
    event_handler.on_subscription_update(Box::new(move |_| {
        let mut c = counter_clone.lock().unwrap();
        *c += 1;
        // Simulate some processing time
        std::thread::sleep(Duration::from_millis(5));
    })).await;
    
    // Process multiple events concurrently
    let mut handles = Vec::new();
    for _ in 0..10 {
        let handler = event_handler.clone();
        let handle = tokio::spawn(async move {
            let msg = WsMessage::SubscriptionUpdated {
                tier: "pro".to_string(),
                status: "active".to_string(),
                billing_period: "monthly".to_string(),
                limits: retrosave::sync::websocket::SubscriptionLimits {
                    saves: Some(10000),
                    storage_gb: 50,
                    devices: 5,
                    family_members: 0,
                },
            };
            handler.handle_websocket_message(msg).await;
        });
        handles.push(handle);
    }
    
    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    sleep(Duration::from_millis(100)).await;
    
    // All events should be processed
    assert_eq!(*counter.lock().unwrap(), 10);
}