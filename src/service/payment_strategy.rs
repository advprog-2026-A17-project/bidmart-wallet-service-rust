use crate::payment::{
    GatewayError, MidtransGateway, PaymentPage, PayoutResult, ValidatedBankAccount,
};
use crate::wallet::Money;

#[tonic::async_trait]
pub trait PaymentGateway: Send + Sync {
    async fn create_payment(
        &self,
        payment_id: &str,
        amount: Money,
        payment_method: Option<&str>,
    ) -> Result<PaymentPage, GatewayError>;

    async fn validate_bank_account(
        &self,
        bank_code: &str,
        account_number: &str,
    ) -> Result<ValidatedBankAccount, GatewayError>;

    async fn create_payout(
        &self,
        user_id: &str,
        amount: Money,
        account: &ValidatedBankAccount,
    ) -> Result<PayoutResult, GatewayError>;

    async fn fetch_transaction_status(&self, payment_id: &str) -> Result<String, GatewayError>;
}

pub struct MidtransPaymentGateway {
    inner: MidtransGateway,
}

impl MidtransPaymentGateway {
    pub fn from_env() -> Self {
        Self {
            inner: MidtransGateway::from_env(),
        }
    }
}

#[tonic::async_trait]
impl PaymentGateway for MidtransPaymentGateway {
    async fn create_payment(
        &self,
        payment_id: &str,
        amount: Money,
        payment_method: Option<&str>,
    ) -> Result<PaymentPage, GatewayError> {
        self.inner
            .create_payment(payment_id, amount, payment_method)
            .await
    }

    async fn validate_bank_account(
        &self,
        bank_code: &str,
        account_number: &str,
    ) -> Result<ValidatedBankAccount, GatewayError> {
        self.inner
            .validate_bank_account(bank_code, account_number)
            .await
    }

    async fn create_payout(
        &self,
        user_id: &str,
        amount: Money,
        account: &ValidatedBankAccount,
    ) -> Result<PayoutResult, GatewayError> {
        self.inner.create_payout(user_id, amount, account).await
    }

    async fn fetch_transaction_status(&self, payment_id: &str) -> Result<String, GatewayError> {
        self.inner.fetch_transaction_status(payment_id).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentStatusStrategy {
    Paid,
    Failed,
    Expired,
    Pending,
}

impl PaymentStatusStrategy {
    pub fn parse(status: &str) -> Option<Self> {
        match status {
            "PAID" => Some(Self::Paid),
            "FAILED" => Some(Self::Failed),
            "EXPIRED" => Some(Self::Expired),
            "PENDING" => Some(Self::Pending),
            _ => None,
        }
    }

    pub fn as_wire_value(self) -> &'static str {
        match self {
            Self::Paid => "PAID",
            Self::Failed => "FAILED",
            Self::Expired => "EXPIRED",
            Self::Pending => "PENDING",
        }
    }
}
