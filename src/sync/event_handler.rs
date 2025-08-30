use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, debug, warn};
use crate::payment::{SubscriptionStatus, UsageStats, BackendSubscriptionTier, TierLimits, TierFeatures, TierPrice};
use super::websocket::WsMessage;

/// Callback types for different events
pub type SubscriptionCallback = Arc<dyn Fn(SubscriptionStatus) + Send + Sync>;
pub type UsageCallback = Arc<dyn Fn(UsageStats) + Send + Sync>;
pub type DeviceCallback = Arc<dyn Fn(String, String) + Send + Sync>;
pub type WarningCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Event handler for WebSocket messages
pub struct EventHandler {
    subscription_listeners: Arc<RwLock<Vec<SubscriptionCallback>>>,
    usage_listeners: Arc<RwLock<Vec<UsageCallback>>>,
    device_added_listeners: Arc<RwLock<Vec<DeviceCallback>>>,
    device_removed_listeners: Arc<RwLock<Vec<DeviceCallback>>>,
    warning_listeners: Arc<RwLock<Vec<WarningCallback>>>,
}

impl EventHandler {
    pub fn new() -> Self {
        Self {
            subscription_listeners: Arc::new(RwLock::new(Vec::new())),
            usage_listeners: Arc::new(RwLock::new(Vec::new())),
            device_added_listeners: Arc::new(RwLock::new(Vec::new())),
            device_removed_listeners: Arc::new(RwLock::new(Vec::new())),
            warning_listeners: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    /// Register a callback for subscription updates
    pub async fn on_subscription_update<F>(&self, callback: F)
    where
        F: Fn(SubscriptionStatus) + Send + Sync + 'static,
    {
        let mut listeners = self.subscription_listeners.write().await;
        listeners.push(Arc::new(callback));
    }
    
    /// Register a callback for usage updates
    pub async fn on_usage_update<F>(&self, callback: F)
    where
        F: Fn(UsageStats) + Send + Sync + 'static,
    {
        let mut listeners = self.usage_listeners.write().await;
        listeners.push(Arc::new(callback));
    }
    
    /// Register a callback for device additions
    pub async fn on_device_added<F>(&self, callback: F)
    where
        F: Fn(String, String) + Send + Sync + 'static,
    {
        let mut listeners = self.device_added_listeners.write().await;
        listeners.push(Arc::new(callback));
    }
    
    /// Register a callback for device removals
    pub async fn on_device_removed<F>(&self, callback: F)
    where
        F: Fn(String, String) + Send + Sync + 'static,
    {
        let mut listeners = self.device_removed_listeners.write().await;
        listeners.push(Arc::new(callback));
    }
    
    /// Register a callback for warnings
    pub async fn on_warning<F>(&self, callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        let mut listeners = self.warning_listeners.write().await;
        listeners.push(Arc::new(callback));
    }
    
    /// Handle incoming WebSocket message
    pub async fn handle_message(&self, message: WsMessage) {
        match message {
            WsMessage::SubscriptionUpdated { tier, status, billing_period, limits } => {
                info!("Received subscription update: tier={}, status={}", tier, status);
                
                // Convert to SubscriptionStatus
                let subscription = SubscriptionStatus {
                    tier: BackendSubscriptionTier {
                        id: tier.clone(),
                        name: Self::tier_display_name(&tier),
                        price: Self::tier_price(&tier),
                        limits: TierLimits {
                            saves: limits.saves,
                            storage_gb: limits.storage_gb,
                            devices: limits.devices,
                            family_members: limits.family_members,
                        },
                        features: Self::tier_features(&tier),
                    },
                    status,
                    billing_period,
                    current_period_end: None, // Not included in WebSocket message
                    cancel_at_period_end: false, // Not included in WebSocket message
                    stripe_customer_id: None, // Not included in WebSocket message
                    stripe_subscription_id: None, // Not included in WebSocket message
                };
                
                // Notify all subscription listeners
                let listeners = self.subscription_listeners.read().await;
                for listener in listeners.iter() {
                    listener(subscription.clone());
                }
            }
            
            WsMessage::UsageUpdated { saves_count, saves_limit, storage_bytes, storage_limit_bytes, devices_count, devices_limit } => {
                debug!("Received usage update: saves={}/{}, storage={}/{}", 
                    saves_count, saves_limit, storage_bytes, storage_limit_bytes);
                
                // Convert to UsageStats
                let usage = UsageStats {
                    saves_count,
                    saves_limit,
                    storage_bytes,
                    storage_limit_bytes,
                    devices_count,
                    devices_limit,
                };
                
                // Notify all usage listeners
                let listeners = self.usage_listeners.read().await;
                for listener in listeners.iter() {
                    listener(usage.clone());
                }
            }
            
            WsMessage::DeviceAdded { device_id, device_name, .. } => {
                info!("Device added: {} ({})", device_name, device_id);
                
                let listeners = self.device_added_listeners.read().await;
                for listener in listeners.iter() {
                    listener(device_id.clone(), device_name.clone());
                }
            }
            
            WsMessage::DeviceRemoved { device_id, device_name } => {
                info!("Device removed: {} ({})", device_name, device_id);
                
                let listeners = self.device_removed_listeners.read().await;
                for listener in listeners.iter() {
                    listener(device_id.clone(), device_name.clone());
                }
            }
            
            WsMessage::StorageLimitWarning { message, percentage } => {
                warn!("Storage limit warning: {} ({}% used)", message, percentage);
                
                let listeners = self.warning_listeners.read().await;
                for listener in listeners.iter() {
                    listener(message.clone());
                }
            }
            
            WsMessage::SaveLimitWarning { message, percentage } => {
                warn!("Save limit warning: {} ({}% used)", message, percentage);
                
                let listeners = self.warning_listeners.read().await;
                for listener in listeners.iter() {
                    listener(message.clone());
                }
            }
            
            _ => {
                // Other message types not handled by this event handler
            }
        }
    }
    
    // Helper methods to convert tier info
    fn tier_display_name(tier: &str) -> String {
        match tier {
            "free" => "Free",
            "pro" => "Pro",
            "family" => "Family",
            "lifetime" => "Lifetime",
            _ => tier,
        }.to_string()
    }
    
    fn tier_price(tier: &str) -> TierPrice {
        match tier {
            "free" => TierPrice { monthly: 0.0, yearly: 0.0 },
            "pro" => TierPrice { monthly: 4.99, yearly: 49.99 },
            "family" => TierPrice { monthly: 9.99, yearly: 99.99 },
            "lifetime" => TierPrice { monthly: 199.99, yearly: 199.99 },
            _ => TierPrice { monthly: 0.0, yearly: 0.0 },
        }
    }
    
    fn tier_features(tier: &str) -> TierFeatures {
        match tier {
            "free" => TierFeatures {
                version_history: false,
                priority_sync: false,
                analytics: false,
                api_access: false,
            },
            "pro" | "family" | "lifetime" => TierFeatures {
                version_history: true,
                priority_sync: true,
                analytics: true,
                api_access: tier == "family" || tier == "lifetime",
            },
            _ => TierFeatures {
                version_history: false,
                priority_sync: false,
                analytics: false,
                api_access: false,
            },
        }
    }
}