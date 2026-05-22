use crate::service::wallet_service::ServiceError;
use crate::wallet::{Money, WalletError};

pub struct WalletCommandContext<'a> {
    pub user_id: &'a str,
    pub role: &'a str,
    pub amount: Option<Money>,
    pub correlation_id: Option<&'a str>,
    pub status: Option<&'a str>,
}

pub trait WalletValidationLink {
    fn validate(&self, context: &WalletCommandContext<'_>) -> Result<(), ServiceError>;
}

pub struct WalletValidationChain {
    links: Vec<Box<dyn WalletValidationLink + Send + Sync>>,
}

impl WalletValidationChain {
    pub fn wallet_mutation() -> Self {
        Self {
            links: vec![
                Box::new(IdentityValidationLink),
                Box::new(AmountValidationLink),
            ],
        }
    }

    pub fn hold_command() -> Self {
        Self {
            links: vec![
                Box::new(IdentityValidationLink),
                Box::new(AmountValidationLink),
                Box::new(CorrelationValidationLink),
            ],
        }
    }

    pub fn payment_status() -> Self {
        Self {
            links: vec![Box::new(PaymentStatusValidationLink)],
        }
    }

    pub fn validate(&self, context: &WalletCommandContext<'_>) -> Result<(), ServiceError> {
        for link in &self.links {
            link.validate(context)?;
        }
        Ok(())
    }
}

struct IdentityValidationLink;

impl WalletValidationLink for IdentityValidationLink {
    fn validate(&self, context: &WalletCommandContext<'_>) -> Result<(), ServiceError> {
        if context.user_id.trim().is_empty() || context.role.trim().is_empty() {
            return Err(ServiceError::ForbiddenAccess);
        }
        Ok(())
    }
}

struct AmountValidationLink;

impl WalletValidationLink for AmountValidationLink {
    fn validate(&self, context: &WalletCommandContext<'_>) -> Result<(), ServiceError> {
        if let Some(amount) = context.amount
            && amount.is_zero()
        {
            return Err(ServiceError::Domain(WalletError::InvalidAmount));
        }
        Ok(())
    }
}

struct CorrelationValidationLink;

impl WalletValidationLink for CorrelationValidationLink {
    fn validate(&self, context: &WalletCommandContext<'_>) -> Result<(), ServiceError> {
        if let Some(correlation_id) = context.correlation_id
            && correlation_id.trim().is_empty()
        {
            return Err(ServiceError::HoldFailed(
                "correlation id is required".to_string(),
            ));
        }
        Ok(())
    }
}

struct PaymentStatusValidationLink;

impl WalletValidationLink for PaymentStatusValidationLink {
    fn validate(&self, context: &WalletCommandContext<'_>) -> Result<(), ServiceError> {
        let Some(status) = context.status else {
            return Err(ServiceError::InvalidPaymentStatus(
                "missing status".to_string(),
            ));
        };
        if !matches!(status, "PAID" | "FAILED" | "EXPIRED" | "PENDING") {
            return Err(ServiceError::InvalidPaymentStatus(status.to_string()));
        }
        Ok(())
    }
}
