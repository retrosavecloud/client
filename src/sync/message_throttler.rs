use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::debug;
use serde_json::Value;

/// Configuration for message throttling
#[derive(Debug, Clone)]
pub struct ThrottleConfig {
    /// Minimum time between UI updates for the same event type
    pub ui_update_interval: Duration,
    /// Time window for deduplication
    pub dedup_window: Duration,
    /// Maximum messages to batch before forcing flush
    pub batch_size: usize,
    /// Time to wait before flushing partial batch
    pub batch_timeout: Duration,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            ui_update_interval: Duration::from_millis(100),
            dedup_window: Duration::from_millis(500),
            batch_size: 10,
            batch_timeout: Duration::from_millis(50),
        }
    }
}

/// Message deduplicator to prevent duplicate processing
struct MessageDeduplicator {
    seen_hashes: HashMap<u64, Instant>,
    window: Duration,
}

impl MessageDeduplicator {
    fn new(window: Duration) -> Self {
        Self {
            seen_hashes: HashMap::new(),
            window,
        }
    }

    fn should_process(&mut self, message: &Value) -> bool {
        // Calculate hash of the message
        let hash = self.calculate_hash(message);
        let now = Instant::now();

        // Clean up old entries
        self.seen_hashes.retain(|_, timestamp| {
            now.duration_since(*timestamp) < self.window
        });

        // Check if we've seen this message recently
        if let Some(last_seen) = self.seen_hashes.get(&hash) {
            if now.duration_since(*last_seen) < self.window {
                debug!("Duplicate message detected, skipping");
                return false;
            }
        }

        // Record this message
        self.seen_hashes.insert(hash, now);
        true
    }

    fn calculate_hash(&self, value: &Value) -> u64 {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;

        let mut hasher = DefaultHasher::new();
        value.to_string().hash(&mut hasher);
        hasher.finish()
    }
}

/// UI update throttler to prevent excessive updates
struct UiThrottler {
    last_updates: HashMap<String, Instant>,
    interval: Duration,
}

impl UiThrottler {
    fn new(interval: Duration) -> Self {
        Self {
            last_updates: HashMap::new(),
            interval,
        }
    }

    fn should_update(&mut self, event_type: &str) -> bool {
        let now = Instant::now();

        if let Some(last_update) = self.last_updates.get(event_type) {
            if now.duration_since(*last_update) < self.interval {
                return false;
            }
        }

        self.last_updates.insert(event_type.to_string(), now);
        true
    }

    fn force_update(&mut self, event_type: &str) {
        self.last_updates.remove(event_type);
    }
}

/// Message batch accumulator
struct MessageBatch {
    messages: Vec<Value>,
    created_at: Instant,
}

impl MessageBatch {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            created_at: Instant::now(),
        }
    }

    fn add(&mut self, message: Value) {
        self.messages.push(message);
    }

    fn should_flush(&self, max_size: usize, timeout: Duration) -> bool {
        self.messages.len() >= max_size || 
        self.created_at.elapsed() > timeout
    }

    fn take(&mut self) -> Vec<Value> {
        let messages = std::mem::take(&mut self.messages);
        self.created_at = Instant::now();
        messages
    }
}

/// Main message throttler for WebSocket messages
pub struct MessageThrottler {
    config: ThrottleConfig,
    deduplicator: Arc<Mutex<MessageDeduplicator>>,
    ui_throttler: Arc<Mutex<UiThrottler>>,
    batches: Arc<Mutex<HashMap<String, MessageBatch>>>,
}

impl MessageThrottler {
    pub fn new(config: ThrottleConfig) -> Self {
        Self {
            deduplicator: Arc::new(Mutex::new(
                MessageDeduplicator::new(config.dedup_window)
            )),
            ui_throttler: Arc::new(Mutex::new(
                UiThrottler::new(config.ui_update_interval)
            )),
            batches: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Process incoming message with deduplication
    pub async fn process_incoming(&self, message: Value) -> Option<Value> {
        let mut dedup = self.deduplicator.lock().await;
        
        if dedup.should_process(&message) {
            Some(message)
        } else {
            None
        }
    }

    /// Check if UI should be updated for this event type
    pub async fn should_update_ui(&self, event_type: &str) -> bool {
        let mut throttler = self.ui_throttler.lock().await;
        throttler.should_update(event_type)
    }

    /// Force immediate UI update for critical events
    pub async fn force_ui_update(&self, event_type: &str) {
        let mut throttler = self.ui_throttler.lock().await;
        throttler.force_update(event_type);
    }

    /// Add message to batch for outgoing messages
    pub async fn batch_outgoing(&self, event_type: String, message: Value) -> Option<Vec<Value>> {
        let mut batches = self.batches.lock().await;
        
        let batch = batches.entry(event_type).or_insert_with(MessageBatch::new);
        batch.add(message);

        if batch.should_flush(self.config.batch_size, self.config.batch_timeout) {
            Some(batch.take())
        } else {
            None
        }
    }

    /// Force flush all batches
    pub async fn flush_all(&self) -> HashMap<String, Vec<Value>> {
        let mut batches = self.batches.lock().await;
        let mut flushed = HashMap::new();

        for (event_type, batch) in batches.iter_mut() {
            if !batch.messages.is_empty() {
                flushed.insert(event_type.clone(), batch.take());
            }
        }

        flushed
    }

    /// Get statistics
    pub async fn get_stats(&self) -> ThrottlerStats {
        let dedup = self.deduplicator.lock().await;
        let ui_throttler = self.ui_throttler.lock().await;
        let batches = self.batches.lock().await;

        ThrottlerStats {
            deduplicated_count: dedup.seen_hashes.len(),
            throttled_types: ui_throttler.last_updates.len(),
            pending_batches: batches.len(),
            pending_messages: batches.values().map(|b| b.messages.len()).sum(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThrottlerStats {
    pub deduplicated_count: usize,
    pub throttled_types: usize,
    pub pending_batches: usize,
    pub pending_messages: usize,
}

/// Priority-based message processor
pub struct PriorityProcessor {
    high_priority_types: HashSet<String>,
    throttler: Arc<MessageThrottler>,
}

impl PriorityProcessor {
    pub fn new(throttler: Arc<MessageThrottler>) -> Self {
        let mut high_priority = HashSet::new();
        // Define high priority event types that should bypass throttling
        high_priority.insert("subscription_updated".to_string());
        high_priority.insert("error".to_string());
        high_priority.insert("auth".to_string());
        high_priority.insert("storage_limit_warning".to_string());
        high_priority.insert("save_limit_warning".to_string());

        Self {
            high_priority_types: high_priority,
            throttler,
        }
    }

    pub async fn process(&self, event_type: &str, message: Value) -> ProcessResult {
        // High priority messages bypass throttling
        if self.high_priority_types.contains(event_type) {
            self.throttler.force_ui_update(event_type).await;
            return ProcessResult::Immediate(message);
        }

        // Check deduplication
        if let Some(message) = self.throttler.process_incoming(message).await {
            // Check UI throttling
            if self.throttler.should_update_ui(event_type).await {
                ProcessResult::Normal(message)
            } else {
                ProcessResult::Throttled
            }
        } else {
            ProcessResult::Duplicate
        }
    }
}

#[derive(Debug)]
pub enum ProcessResult {
    Immediate(Value),  // Process immediately (high priority)
    Normal(Value),     // Process normally
    Throttled,         // Throttled, skip UI update
    Duplicate,         // Duplicate message, skip entirely
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_deduplication() {
        let config = ThrottleConfig {
            dedup_window: Duration::from_millis(100),
            ..Default::default()
        };
        let throttler = MessageThrottler::new(config);

        let msg = serde_json::json!({
            "type": "test",
            "data": "hello"
        });

        // First message should pass
        assert!(throttler.process_incoming(msg.clone()).await.is_some());
        
        // Duplicate should be blocked
        assert!(throttler.process_incoming(msg.clone()).await.is_none());

        // After window, should pass again
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(throttler.process_incoming(msg.clone()).await.is_some());
    }

    #[tokio::test]
    async fn test_ui_throttling() {
        let config = ThrottleConfig {
            ui_update_interval: Duration::from_millis(100),
            ..Default::default()
        };
        let throttler = MessageThrottler::new(config);

        // First update should pass
        assert!(throttler.should_update_ui("test").await);
        
        // Rapid second update should be throttled
        assert!(!throttler.should_update_ui("test").await);

        // Different event type should pass
        assert!(throttler.should_update_ui("other").await);

        // After interval, should pass again
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(throttler.should_update_ui("test").await);
    }

    #[tokio::test]
    async fn test_message_batching() {
        let config = ThrottleConfig {
            batch_size: 3,
            batch_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let throttler = MessageThrottler::new(config);

        let msg = serde_json::json!({"test": "data"});

        // Add messages
        assert!(throttler.batch_outgoing("test".to_string(), msg.clone()).await.is_none());
        assert!(throttler.batch_outgoing("test".to_string(), msg.clone()).await.is_none());
        
        // Third message triggers batch
        let batch = throttler.batch_outgoing("test".to_string(), msg.clone()).await;
        assert!(batch.is_some());
        assert_eq!(batch.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_priority_processing() {
        let throttler = Arc::new(MessageThrottler::new(ThrottleConfig::default()));
        let processor = PriorityProcessor::new(throttler);

        // High priority message should be immediate
        let high_pri_msg = serde_json::json!({"important": true});
        match processor.process("subscription_updated", high_pri_msg).await {
            ProcessResult::Immediate(_) => {}
            _ => panic!("Expected immediate processing"),
        }

        // Normal message follows regular flow
        let normal_msg = serde_json::json!({"normal": true});
        match processor.process("save_uploaded", normal_msg).await {
            ProcessResult::Normal(_) => {}
            _ => panic!("Expected normal processing"),
        }
    }
}