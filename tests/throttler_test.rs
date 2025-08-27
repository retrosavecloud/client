use retrosave::sync::{MessageThrottler, ThrottleConfig, PriorityProcessor};
use std::sync::Arc;
use std::time::Duration;
use serde_json::json;

#[tokio::test]
async fn test_message_deduplication() {
    let config = ThrottleConfig {
        dedup_window: Duration::from_millis(100),
        ..Default::default()
    };
    let throttler = MessageThrottler::new(config);

    // Create identical messages
    let msg = json!({
        "type": "subscription_updated",
        "tier": "pro",
        "status": "active"
    });

    // First message should pass
    assert!(throttler.process_incoming(msg.clone()).await.is_some());
    
    // Immediate duplicate should be blocked
    assert!(throttler.process_incoming(msg.clone()).await.is_none());

    // Different message should pass
    let different_msg = json!({
        "type": "usage_updated",
        "saves_count": 10
    });
    assert!(throttler.process_incoming(different_msg).await.is_some());

    // After dedup window, original message should pass again
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(throttler.process_incoming(msg.clone()).await.is_some());
}

#[tokio::test]
async fn test_ui_throttling() {
    let config = ThrottleConfig {
        ui_update_interval: Duration::from_millis(50),
        ..Default::default()
    };
    let throttler = MessageThrottler::new(config);

    // First update should pass
    assert!(throttler.should_update_ui("subscription").await);
    
    // Rapid second update should be throttled
    assert!(!throttler.should_update_ui("subscription").await);
    
    // Different event type should pass
    assert!(throttler.should_update_ui("usage").await);

    // After throttle interval, should pass again
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(throttler.should_update_ui("subscription").await);
}

#[tokio::test]
async fn test_message_batching() {
    let config = ThrottleConfig {
        batch_size: 3,
        batch_timeout: Duration::from_millis(100),
        ..Default::default()
    };
    let throttler = MessageThrottler::new(config);

    // Add messages to batch
    let msg1 = json!({"id": 1});
    let msg2 = json!({"id": 2});
    let msg3 = json!({"id": 3});

    assert!(throttler.batch_outgoing("test".to_string(), msg1).await.is_none());
    assert!(throttler.batch_outgoing("test".to_string(), msg2).await.is_none());
    
    // Third message should trigger batch flush
    let batch = throttler.batch_outgoing("test".to_string(), msg3).await;
    assert!(batch.is_some());
    assert_eq!(batch.unwrap().len(), 3);
}

#[tokio::test]
async fn test_batch_timeout() {
    let config = ThrottleConfig {
        batch_size: 10,
        batch_timeout: Duration::from_millis(50),
        ..Default::default()
    };
    let throttler = MessageThrottler::new(config);

    // Add single message
    let msg = json!({"test": "data"});
    assert!(throttler.batch_outgoing("test".to_string(), msg.clone()).await.is_none());

    // Wait past timeout
    tokio::time::sleep(Duration::from_millis(60)).await;
    
    // Next message should trigger flush due to timeout
    let batch = throttler.batch_outgoing("test".to_string(), msg).await;
    assert!(batch.is_some());
    assert_eq!(batch.unwrap().len(), 2);
}

#[tokio::test]
async fn test_priority_processing() {
    let throttler = Arc::new(MessageThrottler::new(ThrottleConfig::default()));
    let processor = PriorityProcessor::new(throttler);

    // High priority message should be immediate
    let high_pri = json!({"critical": "data"});
    match processor.process("subscription_updated", high_pri).await {
        retrosave::sync::message_throttler::ProcessResult::Immediate(_) => {}
        _ => panic!("Expected immediate processing for high priority"),
    }

    // Error messages should be immediate
    let error = json!({"error": "something went wrong"});
    match processor.process("error", error).await {
        retrosave::sync::message_throttler::ProcessResult::Immediate(_) => {}
        _ => panic!("Expected immediate processing for error"),
    }

    // Normal message follows regular processing
    let normal = json!({"regular": "data"});
    match processor.process("save_uploaded", normal).await {
        retrosave::sync::message_throttler::ProcessResult::Normal(_) => {}
        _ => panic!("Expected normal processing"),
    }
}

#[tokio::test]
async fn test_duplicate_high_priority() {
    let throttler = Arc::new(MessageThrottler::new(ThrottleConfig {
        dedup_window: Duration::from_millis(100),
        ..Default::default()
    }));
    let processor = PriorityProcessor::new(throttler);

    // Even high priority messages can be duplicates
    let msg = json!({"important": "alert"});
    
    // First should be immediate
    match processor.process("subscription_updated", msg.clone()).await {
        retrosave::sync::message_throttler::ProcessResult::Immediate(_) => {}
        _ => panic!("Expected immediate"),
    }

    // Duplicate should still be caught
    match processor.process("subscription_updated", msg).await {
        retrosave::sync::message_throttler::ProcessResult::Duplicate => {}
        _ => panic!("Expected duplicate detection"),
    }
}

#[tokio::test]
async fn test_force_ui_update() {
    let config = ThrottleConfig {
        ui_update_interval: Duration::from_secs(10), // Long interval
        ..Default::default()
    };
    let throttler = MessageThrottler::new(config);

    // First update passes
    assert!(throttler.should_update_ui("test").await);
    
    // Second would be throttled
    assert!(!throttler.should_update_ui("test").await);
    
    // Force update clears throttling
    throttler.force_ui_update("test").await;
    assert!(throttler.should_update_ui("test").await);
}

#[tokio::test]
async fn test_flush_all_batches() {
    let config = ThrottleConfig {
        batch_size: 10,
        batch_timeout: Duration::from_secs(10),
        ..Default::default()
    };
    let throttler = MessageThrottler::new(config);

    // Add messages to different batches
    throttler.batch_outgoing("type1".to_string(), json!({"a": 1})).await;
    throttler.batch_outgoing("type1".to_string(), json!({"a": 2})).await;
    throttler.batch_outgoing("type2".to_string(), json!({"b": 1})).await;

    // Force flush all
    let flushed = throttler.flush_all().await;
    assert_eq!(flushed.len(), 2);
    assert_eq!(flushed.get("type1").unwrap().len(), 2);
    assert_eq!(flushed.get("type2").unwrap().len(), 1);
}

#[tokio::test]
async fn test_throttler_stats() {
    let throttler = MessageThrottler::new(ThrottleConfig::default());

    // Generate some activity
    let msg = json!({"test": "data"});
    throttler.process_incoming(msg.clone()).await;
    throttler.process_incoming(msg.clone()).await; // Duplicate
    throttler.should_update_ui("test").await;
    throttler.batch_outgoing("batch1".to_string(), msg).await;

    // Check stats
    let stats = throttler.get_stats().await;
    assert!(stats.deduplicated_count > 0);
    assert!(stats.throttled_types > 0);
    assert!(stats.pending_batches > 0);
}

#[tokio::test]
async fn test_concurrent_throttling() {
    let throttler = Arc::new(MessageThrottler::new(ThrottleConfig {
        dedup_window: Duration::from_millis(100),
        ui_update_interval: Duration::from_millis(50),
        ..Default::default()
    }));

    // Spawn multiple tasks processing messages concurrently
    let mut handles = Vec::new();
    
    for i in 0..10 {
        let t = throttler.clone();
        let handle = tokio::spawn(async move {
            let msg = json!({"id": i});
            t.process_incoming(msg).await.is_some()
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let results: Vec<bool> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    // All different messages should pass (no duplicates)
    assert_eq!(results.iter().filter(|&&r| r).count(), 10);
}

#[tokio::test]
async fn test_warning_priority() {
    let throttler = Arc::new(MessageThrottler::new(ThrottleConfig::default()));
    let processor = PriorityProcessor::new(throttler);

    // Storage warnings should be high priority
    let storage_warning = json!({
        "percentage": 90.0,
        "message": "Storage nearly full"
    });
    
    match processor.process("storage_limit_warning", storage_warning).await {
        retrosave::sync::message_throttler::ProcessResult::Immediate(_) => {}
        _ => panic!("Storage warnings should be immediate"),
    }

    // Save limit warnings should be high priority
    let save_warning = json!({
        "percentage": 95.0,
        "message": "Save limit nearly reached"
    });
    
    match processor.process("save_limit_warning", save_warning).await {
        retrosave::sync::message_throttler::ProcessResult::Immediate(_) => {}
        _ => panic!("Save warnings should be immediate"),
    }
}