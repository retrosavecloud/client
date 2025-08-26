pub mod stripe;
pub mod subscription;
pub mod limits;

use serde::{Deserialize, Serialize};
use std::fmt;

/// Subscription tiers available
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubscriptionTier {
    Free,
    Pro,
    Family,
    Lifetime,
}

impl SubscriptionTier {
    pub fn display_name(&self) -> &str {
        match self {
            Self::Free => "Free",
            Self::Pro => "Pro",
            Self::Family => "Family",
            Self::Lifetime => "Lifetime",
        }
    }
    
    pub fn price_monthly(&self) -> f64 {
        match self {
            Self::Free => 0.0,
            Self::Pro => 4.99,
            Self::Family => 9.99,
            Self::Lifetime => 0.0, // One-time payment
        }
    }
    
    pub fn price_yearly(&self) -> f64 {
        match self {
            Self::Free => 0.0,
            Self::Pro => 49.99,
            Self::Family => 99.99,
            Self::Lifetime => 149.99, // One-time payment
        }
    }
    
    pub fn max_saves(&self) -> usize {
        match self {
            Self::Free => 100,
            Self::Pro => 10000,
            Self::Family => 50000,
            Self::Lifetime => usize::MAX,
        }
    }
    
    pub fn max_storage_gb(&self) -> usize {
        match self {
            Self::Free => 1,
            Self::Pro => 50,
            Self::Family => 200,
            Self::Lifetime => 1000,
        }
    }
    
    pub fn max_devices(&self) -> usize {
        match self {
            Self::Free => 2,
            Self::Pro => 5,
            Self::Family => 10,
            Self::Lifetime => 20,
        }
    }
    
    pub fn family_members(&self) -> usize {
        match self {
            Self::Free => 1,
            Self::Pro => 1,
            Self::Family => 5,
            Self::Lifetime => 5,
        }
    }
    
    pub fn features(&self) -> Vec<&str> {
        match self {
            Self::Free => vec![
                "100 save slots",
                "1 GB storage",
                "2 devices",
                "Manual sync",
                "Community support",
            ],
            Self::Pro => vec![
                "10,000 save slots",
                "50 GB storage",
                "5 devices",
                "Automatic sync",
                "Priority support",
                "Version history (30 days)",
                "Cloud backup",
            ],
            Self::Family => vec![
                "50,000 save slots",
                "200 GB storage",
                "10 devices",
                "5 family members",
                "Automatic sync",
                "Priority support",
                "Version history (90 days)",
                "Cloud backup",
                "Family sharing",
            ],
            Self::Lifetime => vec![
                "Unlimited save slots",
                "1 TB storage",
                "20 devices",
                "5 family members",
                "Automatic sync",
                "Premium support",
                "Version history (unlimited)",
                "Cloud backup",
                "Family sharing",
                "Early access features",
                "No recurring fees",
            ],
        }
    }
}

impl fmt::Display for SubscriptionTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Billing period for subscriptions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BillingPeriod {
    Monthly,
    Yearly,
    Lifetime,
}

impl BillingPeriod {
    pub fn display_name(&self) -> &str {
        match self {
            Self::Monthly => "Monthly",
            Self::Yearly => "Yearly",
            Self::Lifetime => "One-time",
        }
    }
    
    pub fn savings_percent(&self) -> u8 {
        match self {
            Self::Monthly => 0,
            Self::Yearly => 17, // ~2 months free
            Self::Lifetime => 100, // No recurring
        }
    }
}

/// User subscription status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionStatus {
    pub tier: SubscriptionTier,
    pub billing_period: BillingPeriod,
    pub status: SubscriptionState,
    pub current_period_end: Option<chrono::DateTime<chrono::Utc>>,
    pub cancel_at_period_end: bool,
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubscriptionState {
    Active,
    Trialing,
    PastDue,
    Canceled,
    Incomplete,
    IncompleteExpired,
    Unpaid,
}

impl SubscriptionStatus {
    pub fn new_free() -> Self {
        Self {
            tier: SubscriptionTier::Free,
            billing_period: BillingPeriod::Monthly,
            status: SubscriptionState::Active,
            current_period_end: None,
            cancel_at_period_end: false,
            stripe_customer_id: None,
            stripe_subscription_id: None,
        }
    }
    
    pub fn is_active(&self) -> bool {
        matches!(self.status, SubscriptionState::Active | SubscriptionState::Trialing)
    }
    
    pub fn days_remaining(&self) -> Option<i64> {
        self.current_period_end.map(|end| {
            let now = chrono::Utc::now();
            (end - now).num_days()
        })
    }
    
    pub fn needs_payment_update(&self) -> bool {
        matches!(self.status, 
            SubscriptionState::PastDue | 
            SubscriptionState::Incomplete | 
            SubscriptionState::Unpaid
        )
    }
}

/// Payment method information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentMethod {
    pub id: String,
    pub card_brand: String,
    pub card_last4: String,
    pub exp_month: u32,
    pub exp_year: u32,
    pub is_default: bool,
}

/// Invoice information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub id: String,
    pub number: String,
    pub amount: f64,
    pub currency: String,
    pub status: String,
    pub created: chrono::DateTime<chrono::Utc>,
    pub pdf_url: Option<String>,
}

/// Usage statistics for the current period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    pub saves_count: usize,
    pub saves_limit: usize,
    pub storage_bytes: u64,
    pub storage_limit_bytes: u64,
    pub devices_count: usize,
    pub devices_limit: usize,
}

impl UsageStats {
    pub fn saves_percentage(&self) -> f32 {
        if self.saves_limit == 0 {
            return 0.0;
        }
        (self.saves_count as f32 / self.saves_limit as f32) * 100.0
    }
    
    pub fn storage_percentage(&self) -> f32 {
        if self.storage_limit_bytes == 0 {
            return 0.0;
        }
        (self.storage_bytes as f32 / self.storage_limit_bytes as f32) * 100.0
    }
    
    pub fn is_near_limit(&self) -> bool {
        self.saves_percentage() > 80.0 || self.storage_percentage() > 80.0
    }
    
    pub fn has_exceeded_limits(&self) -> bool {
        self.saves_count > self.saves_limit || 
        self.storage_bytes > self.storage_limit_bytes ||
        self.devices_count > self.devices_limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_tiers() {
        assert_eq!(SubscriptionTier::Free.price_monthly(), 0.0);
        assert_eq!(SubscriptionTier::Pro.price_monthly(), 4.99);
        assert_eq!(SubscriptionTier::Family.price_yearly(), 99.99);
        assert_eq!(SubscriptionTier::Lifetime.price_yearly(), 149.99);
    }

    #[test]
    fn test_tier_limits() {
        assert_eq!(SubscriptionTier::Free.max_saves(), 100);
        assert_eq!(SubscriptionTier::Pro.max_storage_gb(), 50);
        assert_eq!(SubscriptionTier::Family.max_devices(), 10);
        assert_eq!(SubscriptionTier::Family.family_members(), 5);
    }

    #[test]
    fn test_subscription_status() {
        let status = SubscriptionStatus::new_free();
        assert_eq!(status.tier, SubscriptionTier::Free);
        assert!(status.is_active());
        assert!(!status.needs_payment_update());
    }

    #[test]
    fn test_usage_stats() {
        let stats = UsageStats {
            saves_count: 80,
            saves_limit: 100,
            storage_bytes: 900_000_000, // 900 MB
            storage_limit_bytes: 1_073_741_824, // 1 GB
            devices_count: 2,
            devices_limit: 2,
        };
        
        assert_eq!(stats.saves_percentage(), 80.0);
        assert!(stats.storage_percentage() > 80.0);
        assert!(stats.is_near_limit());
        assert!(!stats.has_exceeded_limits());
    }
}