use sqlx::{FromRow, Row, any::AnyRow};

/// Database row for the wallets table.
#[derive(Debug, FromRow)]
pub struct WalletRow {
    pub id: String,
    pub user_id: String,
    pub role: String,
    pub active_balance: i64,
    pub held_balance: i64,
    pub version: i64,
}

/// Database row for the wallet_transactions table.
#[derive(Debug)]
pub struct TransactionRow {
    pub id: String,
    pub user_id: String,
    pub role: String,
    pub transaction_type: String,
    pub amount: i64,
    pub created_at: String,
    pub correlation_id: Option<String>,
    pub source_service: Option<String>,
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

#[derive(Debug)]
pub struct PaymentIntentRow {
    pub id: String,
    pub user_id: String,
    pub role: String,
    pub amount: i64,
    pub status: String,
    pub redirect_url: String,
    pub va_number: Option<String>,
    pub payment_channel: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl<'r> FromRow<'r, AnyRow> for PaymentIntentRow {
    fn from_row(row: &'r AnyRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            user_id: row.try_get("user_id")?,
            role: row.try_get("role")?,
            amount: row.try_get("amount")?,
            status: row.try_get("status")?,
            redirect_url: row.try_get("redirect_url")?,
            va_number: optional_string(row, "va_number")?,
            payment_channel: optional_string(row, "payment_channel")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

#[derive(Debug)]
pub struct WithdrawalRow {
    pub id: String,
    pub user_id: String,
    pub role: String,
    pub amount: i64,
    pub bank_account: String,
    pub bank_code: Option<String>,
    pub account_number: Option<String>,
    pub account_name: Option<String>,
    pub payout_reference: Option<String>,
    pub failure_reason: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl<'r> FromRow<'r, AnyRow> for WithdrawalRow {
    fn from_row(row: &'r AnyRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            user_id: row.try_get("user_id")?,
            role: row.try_get("role")?,
            amount: row.try_get("amount")?,
            bank_account: row.try_get("bank_account")?,
            bank_code: optional_string(row, "bank_code")?,
            account_number: optional_string(row, "account_number")?,
            account_name: optional_string(row, "account_name")?,
            payout_reference: optional_string(row, "payout_reference")?,
            failure_reason: optional_string(row, "failure_reason")?,
            status: row.try_get("status")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

fn optional_string(row: &AnyRow, column: &str) -> Result<Option<String>, sqlx::Error> {
    match row.try_get(column) {
        Ok(value) => Ok(Some(value)),
        Err(sqlx::Error::ColumnDecode { source, .. })
            if source.to_string().contains("SQL type `NULL`") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

impl<'r> FromRow<'r, AnyRow> for TransactionRow {
    fn from_row(row: &'r AnyRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            user_id: row.try_get("user_id")?,
            role: row.try_get("role")?,
            transaction_type: row.try_get("transaction_type")?,
            amount: row.try_get("amount")?,
            created_at: row.try_get("created_at")?,
            correlation_id: optional_string(row, "correlation_id")?,
            source_service: optional_string(row, "source_service")?,
        })
    }
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

impl From<PaymentIntentRow> for crate::wallet::PaymentIntent {
    fn from(row: PaymentIntentRow) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            role: row.role,
            amount: row.amount,
            status: row.status,
            redirect_url: row.redirect_url,
            va_number: row.va_number,
            payment_channel: row.payment_channel,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl From<WithdrawalRow> for crate::wallet::WalletWithdrawal {
    fn from(row: WithdrawalRow) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            role: row.role,
            amount: row.amount,
            bank_account: row.bank_account,
            bank_code: row.bank_code,
            account_number: row.account_number,
            account_name: row.account_name,
            payout_reference: row.payout_reference,
            failure_reason: row.failure_reason,
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
