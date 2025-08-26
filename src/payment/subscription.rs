use super::{SubscriptionTier, BillingPeriod, SubscriptionStatus, SubscriptionState, UsageStats};
use crate::payment::stripe::StripeClient;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, debug, warn};

/// Manages user subscription state and operations
pub struct SubscriptionManager {
    stripe_client: Arc<RwLock<StripeClient>>,
    current_status: Arc<RwLock<SubscriptionStatus>>,
    usage_stats: Arc<RwLock<UsageStats>>,
}

impl SubscriptionManager {
    pub fn new() -> Self {
        let stripe_client = Arc::new(RwLock::new(StripeClient::new()));
        
        Self {
            stripe_client,
            current_status: Arc::new(RwLock::new(SubscriptionStatus::new_free())),
            usage_stats: Arc::new(RwLock::new(UsageStats::default())),
        }
    }
    
    /// Set authentication token for API requests
    pub async fn set_auth_token(&self, token: Option<String>) {
        self.stripe_client.write().await.set_auth_token(token);
    }
    
    /// Load subscription status from server
    pub async fn load_subscription_status(&self) -> Result<()> {
        debug!("Loading subscription status from backend");
        
        let stripe_client = self.stripe_client.read().await;
        let response = stripe_client.get_subscription_status().await?;
        
        // Convert response to local status
        let tier = match response.tier.as_str() {
            "pro" => SubscriptionTier::Pro,
            "family" => SubscriptionTier::Family,
            "lifetime" => SubscriptionTier::Lifetime,
            _ => SubscriptionTier::Free,
        };
        
        let billing_period = match response.billing_period.as_str() {
            "monthly" => BillingPeriod::Monthly,
            "yearly" => BillingPeriod::Yearly,
            "lifetime" => BillingPeriod::Lifetime,
            _ => BillingPeriod::Monthly,
        };
        
        let state = match response.status.as_str() {
            "active" => SubscriptionState::Active,
            "trialing" => SubscriptionState::Trialing,
            "past_due" => SubscriptionState::PastDue,
            "canceled" => SubscriptionState::Canceled,
            "incomplete" => SubscriptionState::Incomplete,
            _ => SubscriptionState::Active,
        };
        
        let status = SubscriptionStatus {
            tier,
            billing_period,
            status: state,
            current_period_end: response.current_period_end,
            cancel_at_period_end: response.cancel_at_period_end,
            stripe_customer_id: None, // Not exposed from backend
            stripe_subscription_id: None, // Not exposed from backend
        };
        
        *self.current_status.write().await = status;
        
        // Update usage stats from response
        let usage = UsageStats {
            saves_count: response.usage.saves_count,
            saves_limit: response.usage.saves_limit,
            storage_bytes: response.usage.storage_bytes,
            storage_limit_bytes: response.usage.storage_limit_bytes,
            devices_count: response.usage.devices_count,
            devices_limit: response.usage.devices_limit,
        };
        
        *self.usage_stats.write().await = usage;
        
        Ok(())
    }
    
    /// Get current subscription status
    pub async fn get_status(&self) -> SubscriptionStatus {
        self.current_status.read().await.clone()
    }
    
    /// Get current usage statistics
    pub async fn get_usage_stats(&self) -> UsageStats {
        self.usage_stats.read().await.clone()
    }
    
    /// Update usage statistics
    pub async fn update_usage_stats(&self, stats: UsageStats) -> Result<()> {
        *self.usage_stats.write().await = stats;
        
        // Check if approaching limits
        let stats = self.usage_stats.read().await;
        if stats.is_near_limit() {
            warn!("User approaching subscription limits");
            // Would trigger notification to user
        }
        
        Ok(())
    }
    
    /// Start subscription checkout
    pub async fn start_checkout(
        &self,
        tier: SubscriptionTier,
        billing_period: BillingPeriod,
    ) -> Result<String> {
        info!("Starting checkout for {} {}", tier, billing_period.display_name());
        
        let stripe_client = self.stripe_client.read().await;
        let session = stripe_client.create_checkout_session(
            tier,
            billing_period,
        ).await?;
        
        Ok(session.url)
    }
    
    /// Open customer portal for subscription management
    pub async fn open_customer_portal(&self) -> Result<String> {
        let stripe_client = self.stripe_client.read().await;
        let session = stripe_client.create_portal_session().await?;
        
        Ok(session.url)
    }
    
    /// Upgrade subscription
    pub async fn upgrade_subscription(
        &self,
        new_tier: SubscriptionTier,
        new_billing_period: BillingPeriod,
    ) -> Result<()> {
        let status = self.current_status.read().await;
        
        // Check if this is actually an upgrade
        if new_tier as u8 <= status.tier as u8 {
            return Err(anyhow::anyhow!("Can only upgrade to a higher tier"));
        }
        
        info!("Upgrading subscription to {} {}", new_tier, new_billing_period.display_name());
        
        let stripe_client = self.stripe_client.read().await;
        stripe_client.update_subscription(
            new_tier,
            new_billing_period,
        ).await?;
        
        // Update local status
        let mut new_status = self.current_status.write().await;
        new_status.tier = new_tier;
        new_status.billing_period = new_billing_period;
        
        // Reload status from backend to ensure sync
        drop(new_status);
        drop(status);
        self.load_subscription_status().await?;
        
        Ok(())
    }
    
    /// Downgrade subscription
    pub async fn downgrade_subscription(
        &self,
        new_tier: SubscriptionTier,
        new_billing_period: BillingPeriod,
    ) -> Result<()> {
        let status = self.current_status.read().await;
        
        // Check if this is actually a downgrade
        if new_tier as u8 >= status.tier as u8 {
            return Err(anyhow::anyhow!("Can only downgrade to a lower tier"));
        }
        
        // Check if current usage fits in new tier
        let usage = self.usage_stats.read().await;
        if usage.saves_count > new_tier.max_saves() {
            return Err(anyhow::anyhow!(
                "Current save count ({}) exceeds new tier limit ({})",
                usage.saves_count, new_tier.max_saves()
            ));
        }
        
        info!("Downgrading subscription to {} {}", new_tier, new_billing_period.display_name());
        
        let stripe_client = self.stripe_client.read().await;
        stripe_client.update_subscription(
            new_tier,
            new_billing_period,
        ).await?;
        
        // Update local status
        let mut new_status = self.current_status.write().await;
        new_status.tier = new_tier;
        new_status.billing_period = new_billing_period;
        
        // Reload status from backend to ensure sync
        drop(new_status);
        drop(status);
        drop(usage);
        self.load_subscription_status().await?;
        
        Ok(())
    }
    
    /// Cancel subscription
    pub async fn cancel_subscription(&self, immediately: bool) -> Result<()> {
        let status = self.current_status.read().await;
        
        if status.tier == SubscriptionTier::Free {
            return Err(anyhow::anyhow!("No active subscription to cancel"));
        }
        
        info!("Canceling subscription {}", if immediately { "immediately" } else { "at period end" });
        
        let stripe_client = self.stripe_client.read().await;
        stripe_client.cancel_subscription(immediately).await?;
        
        // Update local status
        let mut new_status = self.current_status.write().await;
        if immediately {
            new_status.status = SubscriptionState::Canceled;
            new_status.tier = SubscriptionTier::Free;
        } else {
            new_status.cancel_at_period_end = true;
        }
        
        Ok(())
    }
    
    /// Resume canceled subscription
    pub async fn resume_subscription(&self) -> Result<()> {
        let status = self.current_status.read().await;
        
        if !status.cancel_at_period_end {
            return Err(anyhow::anyhow!("Subscription is not scheduled for cancellation"));
        }
        
        info!("Resuming subscription");
        
        let stripe_client = self.stripe_client.read().await;
        stripe_client.resume_subscription().await?;
        
        // Update local state
        let mut new_status = self.current_status.write().await;
        new_status.cancel_at_period_end = false;
        
        Ok(())
    }
    
    /// Check if user can perform action based on tier limits
    pub async fn can_perform_action(&self, action: SubscriptionAction) -> bool {
        let status = self.current_status.read().await;
        let usage = self.usage_stats.read().await;
        
        match action {
            SubscriptionAction::CreateSave => {
                usage.saves_count < status.tier.max_saves()
            }
            SubscriptionAction::UploadFile(size_bytes) => {
                usage.storage_bytes + size_bytes <= status.tier.max_storage_gb() as u64 * 1_073_741_824
            }
            SubscriptionAction::AddDevice => {
                usage.devices_count < status.tier.max_devices()
            }
            SubscriptionAction::ShareWithFamily => {
                matches!(status.tier, SubscriptionTier::Family | SubscriptionTier::Lifetime)
            }
            SubscriptionAction::AccessVersionHistory => {
                !matches!(status.tier, SubscriptionTier::Free)
            }
        }
    }
    
    /// Get grace period end date if in grace period
    pub async fn get_grace_period_end(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let status = self.current_status.read().await;
        
        if status.status == SubscriptionState::PastDue {
            // Typically 7-14 days after payment failure
            status.current_period_end.map(|end| end + chrono::Duration::days(7))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub enum SubscriptionAction {
    CreateSave,
    UploadFile(u64),
    AddDevice,
    ShareWithFamily,
    AccessVersionHistory,
}

impl Default for UsageStats {
    fn default() -> Self {
        Self {
            saves_count: 0,
            saves_limit: 100, // Free tier default
            storage_bytes: 0,
            storage_limit_bytes: 1_073_741_824, // 1 GB
            devices_count: 1,
            devices_limit: 2,
        }
    }
}

/// Subscription change request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionChangeRequest {
    pub new_tier: SubscriptionTier,
    pub new_billing_period: BillingPeriod,
    pub reason: Option<String>,
    pub effective_date: EffectiveDate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EffectiveDate {
    Immediately,
    EndOfCurrentPeriod,
    SpecificDate(chrono::DateTime<chrono::Utc>),
}

/// Family member in a family plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyMember {
    pub id: String,
    pub email: String,
    pub name: String,
    pub role: FamilyRole,
    pub joined_at: chrono::DateTime<chrono::Utc>,
    pub usage_stats: UsageStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FamilyRole {
    Organizer,
    Member,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_subscription_manager_new() {
        let manager = SubscriptionManager::new();
        
        let status = manager.get_status().await;
        assert_eq!(status.tier, SubscriptionTier::Free);
    }

    #[tokio::test]
    async fn test_can_perform_action() {
        let manager = SubscriptionManager::new();
        
        // Free tier limits
        assert!(manager.can_perform_action(SubscriptionAction::CreateSave).await);
        assert!(!manager.can_perform_action(SubscriptionAction::ShareWithFamily).await);
        assert!(!manager.can_perform_action(SubscriptionAction::AccessVersionHistory).await);
    }

    #[tokio::test]
    async fn test_usage_near_limits() {
        let manager = SubscriptionManager::new();
        
        let stats = UsageStats {
            saves_count: 90,
            saves_limit: 100,
            storage_bytes: 900_000_000,
            storage_limit_bytes: 1_073_741_824,
            devices_count: 2,
            devices_limit: 2,
        };
        
        manager.update_usage_stats(stats).await.unwrap();
        
        let current_stats = manager.get_usage_stats().await;
        assert!(current_stats.is_near_limit());
    }
}