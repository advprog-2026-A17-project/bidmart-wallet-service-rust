use sqlx::FromRow;

/// Database row for the wallets table.
#[derive(Debug, FromRow)]
pub struct WalletRow {
    pub id: String,
    pub user_id: String,
    pub active_balance_cents: i64,
    pub held_balance_cents: i64,
}

/// Database row for the wallet_transactions table.
#[derive(Debug, FromRow)]
pub struct TransactionRow {
    pub id: String,
    pub user_id: String,
    pub transaction_type: String,
    pub amount_cents: i64,
    pub created_at: String,
}

/// Database row for the wallet_provisioning_events table.
#[derive(Debug, FromRow)]
pub struct ProvisioningEventRow {
    pub event_id: String,
    pub user_id: String,
    pub email: String,
    pub occurred_at: String,
    pub source: String,
    pub processed_at: String,
}
