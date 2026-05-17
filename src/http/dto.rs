use serde::{Deserialize, Serialize};

use crate::wallet::{PaymentIntent, Wallet, WalletTransaction, WalletWithdrawal};

// ── Request DTOs ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletCreateRequest {
    pub user_id: String,
    pub role: Option<String>,
    pub active_balance: Option<u64>,
    pub held_balance: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldFundsRequest {
    pub user_id: String,
    pub role: Option<String>,
    pub hold_id: String,
    pub auction_id: String,
    pub bid_id: String,
    pub amount: u64,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseFundsRequest {
    pub hold_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertFundsRequest {
    pub hold_id: String,
}

#[derive(Debug, Deserialize)]
pub struct AmountQuery {
    pub amount: u64,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RoleQuery {
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentIntentRequest {
    pub amount_cents: u64,
    pub role: Option<String>,
    pub payment_method: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawalRequest {
    pub amount_cents: u64,
    pub role: Option<String>,
    pub bank_code: String,
    pub account_number: String,
}

#[derive(Debug, Deserialize)]
pub struct MidtransSimulationRequest {
    pub status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidtransPaymentReturnRequest {
    #[serde(alias = "order_id")]
    pub order_id: String,
    #[serde(alias = "transaction_status")]
    pub transaction_status: String,
    #[serde(alias = "status_code")]
    pub status_code: Option<String>,
}

// ── Response DTOs ───────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletResponse {
    pub id: String,
    pub user_id: String,
    pub role: String,
    pub active_balance: u64,
    pub held_balance: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletTransactionResponse {
    pub id: String,
    pub user_id: String,
    pub role: String,
    #[serde(rename = "type")]
    pub transaction_type: String,
    pub amount: u64,
    pub timestamp: String,
    pub correlation_id: Option<String>,
    pub source_service: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletDetailResponse {
    pub wallet: WalletResponse,
    pub history: Vec<WalletTransactionResponse>,
    pub unpaid_payments: Vec<PaymentIntentResponse>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldResponse {
    pub id: String,
    pub wallet_id: String,
    pub auction_id: String,
    pub bid_id: String,
    pub amount: u64,
    pub status: String,
    pub expires_at: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredErrorResponse {
    pub error_code: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentIntentResponse {
    pub payment_id: String,
    pub amount_cents: u64,
    pub role: String,
    pub status: String,
    pub redirect_url: String,
    pub va_number: Option<String>,
    pub payment_channel: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawalResponse {
    pub withdrawal_id: String,
    pub amount_cents: u64,
    pub role: String,
    pub status: String,
    pub bank_code: Option<String>,
    pub account_number: Option<String>,
    pub account_name: Option<String>,
    pub payout_reference: Option<String>,
}

// ── Conversions ─────────────────────────────────────────────────

impl From<&Wallet> for WalletResponse {
    fn from(w: &Wallet) -> Self {
        Self {
            id: w.id().to_string(),
            user_id: w.user_id().to_string(),
            role: w.role().to_string(),
            active_balance: w.active_balance().cents(),
            held_balance: w.held_balance().cents(),
        }
    }
}

impl From<&WalletTransaction> for WalletTransactionResponse {
    fn from(tx: &WalletTransaction) -> Self {
        Self {
            id: tx.id.clone(),
            user_id: tx.user_id.clone(),
            role: tx.role.clone(),
            transaction_type: tx.transaction_type.as_str().to_string(),
            amount: tx.amount.cents(),
            timestamp: tx.created_at.clone().unwrap_or_default(),
            correlation_id: tx.correlation_id.clone(),
            source_service: tx.source_service.clone(),
        }
    }
}

impl From<&crate::wallet::Hold> for HoldResponse {
    fn from(h: &crate::wallet::Hold) -> Self {
        Self {
            id: h.id.clone(),
            wallet_id: h.wallet_id.clone(),
            auction_id: h.auction_id.clone(),
            bid_id: h.bid_id.clone(),
            amount: h.amount as u64,
            status: h.status.to_string(),
            expires_at: h.expires_at.clone(),
            created_at: h.created_at.clone(),
            updated_at: h.updated_at.clone(),
        }
    }
}

impl From<&PaymentIntent> for PaymentIntentResponse {
    fn from(payment: &PaymentIntent) -> Self {
        Self {
            payment_id: payment.id.clone(),
            amount_cents: payment.amount_cents as u64,
            role: payment.role.clone(),
            status: payment.status.clone(),
            redirect_url: payment.redirect_url.clone(),
            va_number: payment.va_number.clone(),
            payment_channel: payment.payment_channel.clone(),
            created_at: payment.created_at.clone(),
            updated_at: payment.updated_at.clone(),
            expires_at: crate::service::wallet_service::payment_expires_at(&payment.created_at),
        }
    }
}

impl From<&WalletWithdrawal> for WithdrawalResponse {
    fn from(withdrawal: &WalletWithdrawal) -> Self {
        Self {
            withdrawal_id: withdrawal.id.clone(),
            amount_cents: withdrawal.amount_cents as u64,
            role: withdrawal.role.clone(),
            status: withdrawal.status.clone(),
            bank_code: withdrawal.bank_code.clone(),
            account_number: withdrawal.account_number.clone(),
            account_name: withdrawal.account_name.clone(),
            payout_reference: withdrawal.payout_reference.clone(),
        }
    }
}
