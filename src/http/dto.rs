use serde::{Deserialize, Serialize};

use crate::wallet::{Money, Wallet, WalletTransaction};

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
    pub amount: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseFundsRequest {
    pub user_id: String,
    pub amount: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertFundsRequest {
    pub user_id: String,
    pub amount: u64,
}

#[derive(Debug, Deserialize)]
pub struct AmountQuery {
    pub amount: u64,
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
