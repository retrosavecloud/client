use super::{SubscriptionTier, BillingPeriod, PaymentMethod, Invoice};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, debug, warn};
use reqwest::Client;
use std::collections::HashMap;

/// Stripe integration for payment processing
pub struct StripeClient {
    http_client: Client,
    backend_url: String,
    auth_token: Option<String>,
}


impl StripeClient {
    pub fn new() -> Self {
        let backend_url = std::env::var("BACKEND_URL")
            .unwrap_or_else(|_| "https://api.retrosave.cloud".to_string());
        
        Self {
            http_client: Client::builder()
                .cookie_store(true)
                .build()
                .expect("Failed to create HTTP client"),
            backend_url,
            auth_token: None,
        }
    }
    
    /// Set auth token for authenticated requests
    pub fn set_auth_token(&mut self, token: Option<String>) {
        self.auth_token = token;
    }
    
    /// Create a checkout session for subscription
    pub async fn create_checkout_session(
        &self,
        tier: SubscriptionTier,
        billing_period: BillingPeriod,
    ) -> Result<CheckoutSession> {
        debug!("Creating checkout session for {} {}", tier, billing_period.display_name());
        
        let mut request_body = HashMap::new();
        request_body.insert("tier", tier.to_string().to_lowercase());
        request_body.insert("billing_period", match billing_period {
            BillingPeriod::Monthly => "monthly",
            BillingPeriod::Yearly => "yearly",
            BillingPeriod::Lifetime => "lifetime",
        }.to_string());
        
        let response = self.http_client
            .post(format!("{}/api/subscriptions/checkout", self.backend_url))
            .json(&request_body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Failed to create checkout session: {}", error_text));
        }
        
        let session: CheckoutSession = response.json().await?;
        
        // Open the checkout URL in the default browser
        if let Err(e) = webbrowser::open(&session.url) {
            warn!("Failed to open browser: {}", e);
            // Still return the session so user can manually open the URL
        }
        
        Ok(session)
    }
    
    /// Create a customer portal session for subscription management
    pub async fn create_portal_session(&self) -> Result<PortalSession> {
        debug!("Creating portal session");
        
        let response = self.http_client
            .post(format!("{}/api/subscriptions/portal", self.backend_url))
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Failed to create portal session: {}", error_text));
        }
        
        let session: PortalSession = response.json().await?;
        
        // Open the portal URL in the default browser
        if let Err(e) = webbrowser::open(&session.url) {
            warn!("Failed to open browser: {}", e);
        }
        
        Ok(session)
    }
    
    /// Get customer's payment methods
    pub async fn get_payment_methods(&self) -> Result<Vec<PaymentMethod>> {
        debug!("Fetching payment methods");
        
        let response = self.http_client
            .get(format!("{}/api/subscriptions/payment-methods", self.backend_url))
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Failed to fetch payment methods: {}", error_text));
        }
        
        let methods: Vec<PaymentMethod> = response.json().await?;
        Ok(methods)
    }
    
    /// Get customer's invoices
    pub async fn get_invoices(&self, limit: usize) -> Result<Vec<Invoice>> {
        debug!("Fetching invoices with limit {}", limit);
        
        let response = self.http_client
            .get(format!("{}/api/subscriptions/invoices?limit={}", self.backend_url, limit))
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Failed to fetch invoices: {}", error_text));
        }
        
        let invoices: Vec<Invoice> = response.json().await?;
        Ok(invoices)
    }
    
    /// Cancel subscription
    pub async fn cancel_subscription(
        &self,
        cancel_immediately: bool,
    ) -> Result<()> {
        info!("Canceling subscription {}", if cancel_immediately { "immediately" } else { "at period end" });
        
        let mut request_body = HashMap::new();
        request_body.insert("cancel_immediately", cancel_immediately.to_string());
        
        let response = self.http_client
            .post(format!("{}/api/subscriptions/cancel", self.backend_url))
            .json(&request_body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Failed to cancel subscription: {}", error_text));
        }
        
        Ok(())
    }
    
    /// Update subscription (upgrade/downgrade)
    pub async fn update_subscription(
        &self,
        new_tier: SubscriptionTier,
        new_billing_period: BillingPeriod,
    ) -> Result<()> {
        info!("Updating subscription to {} {}", new_tier, new_billing_period.display_name());
        
        let mut request_body = HashMap::new();
        request_body.insert("tier", new_tier.to_string().to_lowercase());
        request_body.insert("billing_period", match new_billing_period {
            BillingPeriod::Monthly => "monthly",
            BillingPeriod::Yearly => "yearly",
            BillingPeriod::Lifetime => "lifetime",
        }.to_string());
        
        let response = self.http_client
            .put(format!("{}/api/subscriptions/update", self.backend_url))
            .json(&request_body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Failed to update subscription: {}", error_text));
        }
        
        Ok(())
    }
    
    /// Resume a canceled subscription
    pub async fn resume_subscription(&self) -> Result<()> {
        info!("Resuming subscription");
        
        let response = self.http_client
            .post(format!("{}/api/subscriptions/resume", self.backend_url))
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Failed to resume subscription: {}", error_text));
        }
        
        Ok(())
    }
    
    /// Get current subscription status from backend
    pub async fn get_subscription_status(&self) -> Result<SubscriptionStatusResponse> {
        debug!("Fetching subscription status");
        
        let response = self.http_client
            .get(format!("{}/api/subscriptions/status", self.backend_url))
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Failed to fetch subscription status: {}", error_text));
        }
        
        let status: SubscriptionStatusResponse = response.json().await?;
        Ok(status)
    }
    
    /// Handle deep link return from Stripe checkout/portal
    pub fn handle_deep_link(&self, url: &str) -> Result<DeepLinkAction> {
        debug!("Handling deep link: {}", url);
        
        if url.contains("success") {
            Ok(DeepLinkAction::CheckoutSuccess)
        } else if url.contains("cancel") {
            Ok(DeepLinkAction::CheckoutCancel)
        } else if url.contains("portal") {
            Ok(DeepLinkAction::PortalReturn)
        } else {
            Ok(DeepLinkAction::Unknown)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckoutSession {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalSession {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionStatusResponse {
    pub tier: String,
    pub billing_period: String,
    pub status: String,
    pub current_period_end: Option<chrono::DateTime<chrono::Utc>>,
    pub cancel_at_period_end: bool,
    pub usage: UsageResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageResponse {
    pub saves_count: usize,
    pub saves_limit: usize,
    pub storage_bytes: u64,
    pub storage_limit_bytes: u64,
    pub devices_count: usize,
    pub devices_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLinkAction {
    CheckoutSuccess,
    CheckoutCancel,
    PortalReturn,
    Unknown,
}

/// Stripe Elements configuration for embedded payment forms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementsConfig {
    pub publishable_key: String,
    pub appearance: ElementsAppearance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementsAppearance {
    pub theme: String,
    pub variables: ElementsVariables,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementsVariables {
    pub color_primary: String,
    pub color_background: String,
    pub color_surface: String,
    pub color_text: String,
    pub font_family: String,
    pub border_radius: String,
}

impl Default for ElementsAppearance {
    fn default() -> Self {
        Self {
            theme: "stripe".to_string(),
            variables: ElementsVariables {
                color_primary: "#635BFF".to_string(),
                color_background: "#ffffff".to_string(),
                color_surface: "#f6f9fc".to_string(),
                color_text: "#32325d".to_string(),
                font_family: "system-ui, sans-serif".to_string(),
                border_radius: "4px".to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stripe_client_new() {
        let client = StripeClient::new();
        assert!(!client.backend_url.is_empty());
    }

    #[test]
    fn test_handle_deep_link() {
        let client = StripeClient::new();
        
        assert_eq!(
            client.handle_deep_link("retrosave://payment/success").unwrap(),
            DeepLinkAction::CheckoutSuccess
        );
        
        assert_eq!(
            client.handle_deep_link("retrosave://payment/cancel").unwrap(),
            DeepLinkAction::CheckoutCancel
        );
        
        assert_eq!(
            client.handle_deep_link("retrosave://portal/return").unwrap(),
            DeepLinkAction::PortalReturn
        );
        
        assert_eq!(
            client.handle_deep_link("retrosave://unknown").unwrap(),
            DeepLinkAction::Unknown
        );
    }
}