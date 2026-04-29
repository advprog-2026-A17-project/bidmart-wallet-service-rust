use sqlx::SqlitePool;

use crate::persistence::models::{TransactionRow, WalletRow};
use crate::wallet::{Money, TransactionType, Wallet, WalletTransaction};

// ── WalletRepository ─────────────────────────────────────────────

pub struct WalletRepository {
    pool: SqlitePool,
}

impl WalletRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, wallet: &Wallet) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO wallets (id, user_id, active_balance_cents, held_balance_cents) VALUES (?, ?, ?, ?)",
        )
        .bind(wallet.id())
        .bind(wallet.user_id())
        .bind(wallet.active_balance().cents() as i64)
        .bind(wallet.held_balance().cents() as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_user_id(&self, user_id: &str) -> Result<Option<Wallet>, sqlx::Error> {
        let row: Option<WalletRow> = sqlx::query_as(
            "SELECT id, user_id, active_balance_cents, held_balance_cents FROM wallets WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| wallet_from_row(r)))
    }

    pub async fn update(&self, wallet: &Wallet) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE wallets SET active_balance_cents = ?, held_balance_cents = ? WHERE id = ?",
        )
        .bind(wallet.active_balance().cents() as i64)
        .bind(wallet.held_balance().cents() as i64)
        .bind(wallet.id())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_all(&self) -> Result<Vec<Wallet>, sqlx::Error> {
        let rows: Vec<WalletRow> = sqlx::query_as(
            "SELECT id, user_id, active_balance_cents, held_balance_cents FROM wallets",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(wallet_from_row).collect())
    }
}

fn wallet_from_row(row: WalletRow) -> Wallet {
    Wallet::with_balances(
        row.id,
        row.user_id,
        Money::from_cents(row.active_balance_cents as u64),
        Money::from_cents(row.held_balance_cents as u64),
    )
}

// ── TransactionRepository ────────────────────────────────────────

pub struct TransactionRepository {
    pool: SqlitePool,
}

impl TransactionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, tx: &WalletTransaction) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO wallet_transactions (id, user_id, transaction_type, amount_cents) VALUES (?, ?, ?, ?)",
        )
        .bind(&tx.id)
        .bind(&tx.user_id)
        .bind(tx.transaction_type.as_str())
        .bind(tx.amount.cents() as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_user_id(
        &self,
        user_id: &str,
    ) -> Result<Vec<WalletTransaction>, sqlx::Error> {
        let rows: Vec<TransactionRow> = sqlx::query_as(
            "SELECT id, user_id, transaction_type, amount_cents, created_at FROM wallet_transactions WHERE user_id = ? ORDER BY created_at DESC, rowid DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(transaction_from_row).collect())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<WalletTransaction>, sqlx::Error> {
        let row: Option<TransactionRow> = sqlx::query_as(
            "SELECT id, user_id, transaction_type, amount_cents, created_at FROM wallet_transactions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(transaction_from_row))
    }
}

fn parse_transaction_type(s: &str) -> TransactionType {
    match s {
        "TOP_UP" => TransactionType::TopUp,
        "WITHDRAW" => TransactionType::Withdraw,
        "HOLD" => TransactionType::Hold,
        "RELEASE" => TransactionType::Release,
        "CONVERT" => TransactionType::Convert,
        "BID" => TransactionType::Bid,
        "CANCEL_BID" => TransactionType::CancelBid,
        other => panic!("unknown transaction type: {other}"),
    }
}

fn transaction_from_row(row: TransactionRow) -> WalletTransaction {
    WalletTransaction {
        id: row.id,
        user_id: row.user_id,
        transaction_type: parse_transaction_type(&row.transaction_type),
        amount: Money::from_cents(row.amount_cents as u64),
    }
}
