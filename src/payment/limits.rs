use super::{SubscriptionTier, UsageStats};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, debug, warn, error};

/// Enforces subscription tier limits
pub struct LimitsEnforcer {
    tier: Arc<RwLock<SubscriptionTier>>,
    usage: Arc<RwLock<UsageStats>>,
    enforcement_mode: EnforcementMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementMode {
    /// Strict: Block actions that exceed limits
    Strict,
    /// Soft: Warn but allow actions temporarily
    Soft,
    /// Grace: Allow with warnings during grace period
    Grace,
}

impl LimitsEnforcer {
    pub fn new(tier: SubscriptionTier, mode: EnforcementMode) -> Self {
        let usage = UsageStats {
            saves_count: 0,
            saves_limit: tier.max_saves(),
            storage_bytes: 0,
            storage_limit_bytes: (tier.max_storage_gb() as u64) * 1_073_741_824,
            devices_count: 0,
            devices_limit: tier.max_devices(),
        };
        
        Self {
            tier: Arc::new(RwLock::new(tier)),
            usage: Arc::new(RwLock::new(usage)),
            enforcement_mode: mode,
        }
    }
    
    /// Update the subscription tier
    pub async fn update_tier(&self, new_tier: SubscriptionTier) {
        *self.tier.write().await = new_tier;
        
        // Update limits in usage stats
        let mut usage = self.usage.write().await;
        usage.saves_limit = new_tier.max_saves();
        usage.storage_limit_bytes = (new_tier.max_storage_gb() as u64) * 1_073_741_824;
        usage.devices_limit = new_tier.max_devices();
    }
    
    /// Check if a save can be created
    pub async fn can_create_save(&self) -> Result<LimitCheckResult> {
        let tier = self.tier.read().await;
        let usage = self.usage.read().await;
        
        if usage.saves_count >= usage.saves_limit {
            match self.enforcement_mode {
                EnforcementMode::Strict => {
                    error!("Save limit exceeded: {}/{}", usage.saves_count, usage.saves_limit);
                    Ok(LimitCheckResult::Blocked {
                        reason: format!("Save limit exceeded. Current: {}, Limit: {}", 
                            usage.saves_count, usage.saves_limit),
                        suggestion: self.get_upgrade_suggestion(*tier),
                    })
                }
                EnforcementMode::Soft | EnforcementMode::Grace => {
                    warn!("Save limit exceeded but allowing: {}/{}", usage.saves_count, usage.saves_limit);
                    Ok(LimitCheckResult::AllowedWithWarning {
                        warning: format!("You've exceeded your save limit ({}/{})", 
                            usage.saves_count, usage.saves_limit),
                        suggestion: self.get_upgrade_suggestion(*tier),
                    })
                }
            }
        } else if usage.saves_count as f32 / usage.saves_limit as f32 > 0.8 {
            Ok(LimitCheckResult::AllowedWithWarning {
                warning: format!("Approaching save limit: {}/{}", usage.saves_count, usage.saves_limit),
                suggestion: None,
            })
        } else {
            Ok(LimitCheckResult::Allowed)
        }
    }
    
    /// Check if a file upload is allowed
    pub async fn can_upload_file(&self, file_size: u64) -> Result<LimitCheckResult> {
        let tier = self.tier.read().await;
        let usage = self.usage.read().await;
        
        let new_total = usage.storage_bytes + file_size;
        
        if new_total > usage.storage_limit_bytes {
            match self.enforcement_mode {
                EnforcementMode::Strict => {
                    error!("Storage limit would be exceeded: {} + {} > {}", 
                        usage.storage_bytes, file_size, usage.storage_limit_bytes);
                    Ok(LimitCheckResult::Blocked {
                        reason: format!("Storage limit would be exceeded. Available: {} bytes", 
                            usage.storage_limit_bytes.saturating_sub(usage.storage_bytes)),
                        suggestion: self.get_upgrade_suggestion(*tier),
                    })
                }
                EnforcementMode::Soft | EnforcementMode::Grace => {
                    warn!("Storage limit would be exceeded but allowing: {} + {} > {}", 
                        usage.storage_bytes, file_size, usage.storage_limit_bytes);
                    Ok(LimitCheckResult::AllowedWithWarning {
                        warning: format!("This upload would exceed your storage limit"),
                        suggestion: self.get_upgrade_suggestion(*tier),
                    })
                }
            }
        } else if new_total as f32 / usage.storage_limit_bytes as f32 > 0.8 {
            Ok(LimitCheckResult::AllowedWithWarning {
                warning: format!("Approaching storage limit: {:.1}% used", 
                    (new_total as f32 / usage.storage_limit_bytes as f32) * 100.0),
                suggestion: None,
            })
        } else {
            Ok(LimitCheckResult::Allowed)
        }
    }
    
    /// Check if a device can be added
    pub async fn can_add_device(&self) -> Result<LimitCheckResult> {
        let tier = self.tier.read().await;
        let usage = self.usage.read().await;
        
        if usage.devices_count >= usage.devices_limit {
            match self.enforcement_mode {
                EnforcementMode::Strict => {
                    error!("Device limit exceeded: {}/{}", usage.devices_count, usage.devices_limit);
                    Ok(LimitCheckResult::Blocked {
                        reason: format!("Device limit reached. Current: {}, Limit: {}", 
                            usage.devices_count, usage.devices_limit),
                        suggestion: self.get_upgrade_suggestion(*tier),
                    })
                }
                EnforcementMode::Soft | EnforcementMode::Grace => {
                    warn!("Device limit exceeded but allowing: {}/{}", usage.devices_count, usage.devices_limit);
                    Ok(LimitCheckResult::AllowedWithWarning {
                        warning: format!("Device limit exceeded ({}/{})", 
                            usage.devices_count, usage.devices_limit),
                        suggestion: self.get_upgrade_suggestion(*tier),
                    })
                }
            }
        } else {
            Ok(LimitCheckResult::Allowed)
        }
    }
    
    /// Record a save creation
    pub async fn record_save_created(&self) -> Result<()> {
        let mut usage = self.usage.write().await;
        usage.saves_count += 1;
        debug!("Save created. Total: {}/{}", usage.saves_count, usage.saves_limit);
        Ok(())
    }
    
    /// Record a save deletion
    pub async fn record_save_deleted(&self) -> Result<()> {
        let mut usage = self.usage.write().await;
        if usage.saves_count > 0 {
            usage.saves_count -= 1;
            debug!("Save deleted. Total: {}/{}", usage.saves_count, usage.saves_limit);
        }
        Ok(())
    }
    
    /// Record file upload
    pub async fn record_file_uploaded(&self, size: u64) -> Result<()> {
        let mut usage = self.usage.write().await;
        usage.storage_bytes += size;
        debug!("File uploaded. Storage: {}/{} bytes", usage.storage_bytes, usage.storage_limit_bytes);
        Ok(())
    }
    
    /// Record file deletion
    pub async fn record_file_deleted(&self, size: u64) -> Result<()> {
        let mut usage = self.usage.write().await;
        usage.storage_bytes = usage.storage_bytes.saturating_sub(size);
        debug!("File deleted. Storage: {}/{} bytes", usage.storage_bytes, usage.storage_limit_bytes);
        Ok(())
    }
    
    /// Record device addition
    pub async fn record_device_added(&self) -> Result<()> {
        let mut usage = self.usage.write().await;
        usage.devices_count += 1;
        debug!("Device added. Total: {}/{}", usage.devices_count, usage.devices_limit);
        Ok(())
    }
    
    /// Record device removal
    pub async fn record_device_removed(&self) -> Result<()> {
        let mut usage = self.usage.write().await;
        if usage.devices_count > 0 {
            usage.devices_count -= 1;
            debug!("Device removed. Total: {}/{}", usage.devices_count, usage.devices_limit);
        }
        Ok(())
    }
    
    /// Get current usage statistics
    pub async fn get_usage(&self) -> UsageStats {
        self.usage.read().await.clone()
    }
    
    /// Set enforcement mode
    pub fn set_enforcement_mode(&mut self, mode: EnforcementMode) {
        self.enforcement_mode = mode;
        info!("Enforcement mode set to: {:?}", mode);
    }
    
    /// Get upgrade suggestion based on current tier
    fn get_upgrade_suggestion(&self, current_tier: SubscriptionTier) -> Option<String> {
        match current_tier {
            SubscriptionTier::Free => {
                Some("Upgrade to Pro for 10,000 saves and 50GB storage".to_string())
            }
            SubscriptionTier::Pro => {
                Some("Upgrade to Family for 50,000 saves and 200GB storage".to_string())
            }
            SubscriptionTier::Family => {
                Some("Consider Lifetime for unlimited saves and 1TB storage".to_string())
            }
            SubscriptionTier::Lifetime => None,
        }
    }
    
    /// Clean up old data to free up space
    pub async fn cleanup_old_data(&self, days_to_keep: u32) -> Result<CleanupResult> {
        info!("Starting cleanup of data older than {} days", days_to_keep);
        
        // In production, this would:
        // 1. Delete old save versions
        // 2. Remove unused cached files
        // 3. Compress old data
        
        // Mock cleanup
        let freed_bytes = 100_000_000; // 100 MB
        let deleted_saves = 10;
        
        let mut usage = self.usage.write().await;
        usage.storage_bytes = usage.storage_bytes.saturating_sub(freed_bytes);
        usage.saves_count = usage.saves_count.saturating_sub(deleted_saves);
        
        Ok(CleanupResult {
            freed_bytes,
            deleted_saves,
            deleted_files: 15,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitCheckResult {
    /// Action is allowed
    Allowed,
    /// Action is allowed but user should be warned
    AllowedWithWarning {
        warning: String,
        suggestion: Option<String>,
    },
    /// Action is blocked
    Blocked {
        reason: String,
        suggestion: Option<String>,
    },
}

impl LimitCheckResult {
    pub fn is_allowed(&self) -> bool {
        !matches!(self, Self::Blocked { .. })
    }
    
    pub fn get_message(&self) -> Option<String> {
        match self {
            Self::Allowed => None,
            Self::AllowedWithWarning { warning, .. } => Some(warning.clone()),
            Self::Blocked { reason, .. } => Some(reason.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupResult {
    pub freed_bytes: u64,
    pub deleted_saves: usize,
    pub deleted_files: usize,
}

/// Quota alert levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaAlert {
    None,
    Approaching(u8), // Percentage used
    Exceeded,
}

impl QuotaAlert {
    pub fn from_usage(used: usize, limit: usize) -> Self {
        if used >= limit {
            Self::Exceeded
        } else {
            let percentage = (used as f32 / limit as f32 * 100.0) as u8;
            if percentage >= 80 {
                Self::Approaching(percentage)
            } else {
                Self::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_limits_enforcer_free_tier() {
        let enforcer = LimitsEnforcer::new(SubscriptionTier::Free, EnforcementMode::Strict);
        
        // Should allow initial saves
        let result = enforcer.can_create_save().await.unwrap();
        assert_eq!(result, LimitCheckResult::Allowed);
        
        // Record saves up to limit
        for _ in 0..100 {
            enforcer.record_save_created().await.unwrap();
        }
        
        // Should block when at limit
        let result = enforcer.can_create_save().await.unwrap();
        assert!(matches!(result, LimitCheckResult::Blocked { .. }));
    }

    #[tokio::test]
    async fn test_storage_limits() {
        let enforcer = LimitsEnforcer::new(SubscriptionTier::Free, EnforcementMode::Strict);
        
        // 1 GB limit for free tier
        let one_gb = 1_073_741_824;
        
        // Should allow small file
        let result = enforcer.can_upload_file(1_000_000).await.unwrap();
        assert!(result.is_allowed());
        
        // Should block file that exceeds limit
        let result = enforcer.can_upload_file(one_gb + 1).await.unwrap();
        assert!(!result.is_allowed());
    }

    #[tokio::test]
    async fn test_grace_mode() {
        let mut enforcer = LimitsEnforcer::new(SubscriptionTier::Free, EnforcementMode::Grace);
        
        // Fill up to limit
        for _ in 0..100 {
            enforcer.record_save_created().await.unwrap();
        }
        
        // Should allow with warning in grace mode
        let result = enforcer.can_create_save().await.unwrap();
        assert!(matches!(result, LimitCheckResult::AllowedWithWarning { .. }));
    }

    #[test]
    fn test_quota_alert() {
        assert_eq!(QuotaAlert::from_usage(0, 100), QuotaAlert::None);
        assert_eq!(QuotaAlert::from_usage(79, 100), QuotaAlert::None);
        assert!(matches!(QuotaAlert::from_usage(80, 100), QuotaAlert::Approaching(80)));
        assert!(matches!(QuotaAlert::from_usage(95, 100), QuotaAlert::Approaching(95)));
        assert_eq!(QuotaAlert::from_usage(100, 100), QuotaAlert::Exceeded);
        assert_eq!(QuotaAlert::from_usage(101, 100), QuotaAlert::Exceeded);
    }
}