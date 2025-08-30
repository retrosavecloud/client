use serde::{Deserialize, Serialize};

/// Subscription tiers available
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
}

/// Billing period for subscriptions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BillingPeriod {
    Monthly,
    Yearly,
    Lifetime,
}

/// Backend subscription tier structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendSubscriptionTier {
    pub id: String,
    pub name: String,
    pub price: TierPrice,
    pub limits: TierLimits,
    pub features: TierFeatures,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierPrice {
    pub monthly: f64,
    pub yearly: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierLimits {
    pub saves: Option<i32>,
    pub storage_gb: i32,
    pub devices: i32,
    pub family_members: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierFeatures {
    pub version_history: bool,
    pub priority_sync: bool,
    pub analytics: bool,
    pub api_access: bool,
}

/// User subscription status (matches backend response)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionStatus {
    pub tier: BackendSubscriptionTier,
    pub status: String, // Backend sends string, not enum
    pub billing_period: String,
    pub current_period_end: Option<String>, // ISO8601 string from backend
    pub cancel_at_period_end: bool,
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
    pub fn is_active(&self) -> bool {
        self.status == "active" || self.status == "trialing"
    }
}


/// Usage statistics for the current period (matches backend response)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    pub saves_count: i32,
    pub saves_limit: i32,
    pub storage_bytes: i64,
    pub storage_limit_bytes: i64,
    pub devices_count: i32,
    pub devices_limit: i32,
}

impl UsageStats {
    pub fn saves_percentage(&self) -> f32 {
        if self.saves_limit <= 0 {
            return 0.0;
        }
        (self.saves_count as f32 / self.saves_limit as f32) * 100.0
    }
    
    pub fn storage_percentage(&self) -> f32 {
        if self.storage_limit_bytes <= 0 {
            return 0.0;
        }
        (self.storage_bytes as f32 / self.storage_limit_bytes as f32) * 100.0
    }
    
    pub fn is_near_limit(&self) -> bool {
        self.saves_percentage() > 80.0 || self.storage_percentage() > 80.0
    }
}

