use serde::{Deserialize, Serialize};

use crate::wallet::{PaymentIntent, Wallet, WalletTransaction, WalletWithdrawal};

// ── Request DTOs ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletCreateRequest {
    pub user_id: String,
    pub active_balance: Option<u64>,
    pub held_balance: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldFundsRequest {
    pub user_id: String,
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentIntentRequest {
    pub amount_cents: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawalRequest {
    pub amount_cents: u64,
    pub bank_account: String,
}

#[derive(Debug, Deserialize)]
pub struct MidtransSimulationRequest {
    pub status: String,
}

// ── Response DTOs ───────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletResponse {
    pub id: String,
    pub user_id: String,
    pub active_balance: u64,
    pub held_balance: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletTransactionResponse {
    pub id: String,
    pub user_id: String,
    #[serde(rename = "type")]
    pub transaction_type: String,
    pub amount: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletDetailResponse {
    pub wallet: WalletResponse,
    pub history: Vec<WalletTransactionResponse>,
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
    pub status: String,
    pub redirect_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawalResponse {
    pub withdrawal_id: String,
    pub amount_cents: u64,
    pub status: String,
}

// ── Conversions ─────────────────────────────────────────────────

impl From<&Wallet> for WalletResponse {
    fn from(w: &Wallet) -> Self {
        Self {
            id: w.id().to_string(),
            user_id: w.user_id().to_string(),
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
            transaction_type: tx.transaction_type.as_str().to_string(),
            amount: tx.amount.cents(),
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
            status: h.status.to_string(), // Menggunakan trait Display yang baru kita buat
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
            status: payment.status.clone(),
            redirect_url: payment.redirect_url.clone(),
        }
    }
}

impl From<&WalletWithdrawal> for WithdrawalResponse {
    fn from(withdrawal: &WalletWithdrawal) -> Self {
        Self {
            withdrawal_id: withdrawal.id.clone(),
            amount_cents: withdrawal.amount_cents as u64,
            status: withdrawal.status.clone(),
        }
    }
}
