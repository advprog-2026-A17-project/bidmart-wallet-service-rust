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

#[derive(Debug, FromRow)]
pub struct HoldRow {
    pub id: String,
    pub wallet_id: String,
    pub auction_id: String,
    pub bid_id: String,
    pub amount: i64, 
    pub status: String,
    pub expires_at: String,
    pub created_at: String,
    pub updated_at: String,
}

impl TryFrom<HoldRow> for crate::wallet::Hold {
    type Error = String;

    fn try_from(row: HoldRow) -> Result<Self, Self::Error> {
        use std::str::FromStr;
        Ok(crate::wallet::Hold {
            id: row.id,
            wallet_id: row.wallet_id,
            auction_id: row.auction_id,
            bid_id: row.bid_id,
            amount: row.amount,
            status: crate::wallet::HoldStatus::from_str(&row.status)?,
            expires_at: row.expires_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}