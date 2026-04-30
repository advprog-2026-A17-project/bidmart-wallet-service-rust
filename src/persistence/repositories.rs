use sqlx::SqlitePool;

use crate::persistence::models::{ProvisioningEventRow, TransactionRow, WalletRow};
use crate::wallet::{Money, TransactionType, Wallet, WalletTransaction};

// ── Column lists (DRY) ──────────────────────────────────────────

const WALLET_COLS: &str = "id, user_id, active_balance_cents, held_balance_cents";
const TX_COLS: &str = "id, user_id, transaction_type, amount_cents, created_at";
const PROV_COLS: &str = "event_id, user_id, email, occurred_at, source, processed_at";

// ── Row → Domain mappers ────────────────────────────────────────

fn wallet_from_row(row: WalletRow) -> Wallet {
    Wallet::with_balances(
        row.id,
        row.user_id,
        Money::from_cents(row.active_balance_cents as u64),
        Money::from_cents(row.held_balance_cents as u64),
    )
}

fn transaction_from_row(row: TransactionRow) -> WalletTransaction {
    WalletTransaction {
        id: row.id,
        user_id: row.user_id,
        transaction_type: TransactionType::from_str(&row.transaction_type),
        amount: Money::from_cents(row.amount_cents as u64),
    }
}

// ── WalletRepository ────────────────────────────────────────────

pub struct WalletRepository {
    pool: SqlitePool,
}

impl WalletRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, wallet: &Wallet) -> Result<(), sqlx::Error> {
        let sql = format!(
            "INSERT INTO wallets ({WALLET_COLS}) VALUES (?, ?, ?, ?)"
        );
        sqlx::query(&sql)
            .bind(wallet.id())
            .bind(wallet.user_id())
            .bind(wallet.active_balance().cents() as i64)
            .bind(wallet.held_balance().cents() as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn find_by_user_id(&self, user_id: &str) -> Result<Option<Wallet>, sqlx::Error> {
        let sql = format!(
            "SELECT {WALLET_COLS} FROM wallets WHERE user_id = ?"
        );
        let row: Option<WalletRow> = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(wallet_from_row))
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
        let sql = format!("SELECT {WALLET_COLS} FROM wallets");
        let rows: Vec<WalletRow> = sqlx::query_as(&sql)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(wallet_from_row).collect())
    }
}

// ── TransactionRepository ───────────────────────────────────────

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
        let sql = format!(
            "SELECT {TX_COLS} FROM wallet_transactions WHERE user_id = ? ORDER BY created_at DESC, rowid DESC"
        );
        let rows: Vec<TransactionRow> = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(transaction_from_row).collect())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<WalletTransaction>, sqlx::Error> {
        let sql = format!(
            "SELECT {TX_COLS} FROM wallet_transactions WHERE id = ?"
        );
        let row: Option<TransactionRow> = sqlx::query_as(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(transaction_from_row))
    }
}

// ── ProvisioningEventRepository ─────────────────────────────────

pub struct ProvisioningEventRepository {
    pool: SqlitePool,
}

impl ProvisioningEventRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn exists(&self, event_id: &str) -> Result<bool, sqlx::Error> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM wallet_provisioning_events WHERE event_id = ?",
        )
        .bind(event_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.is_some())
    }

    pub async fn insert(
        &self,
        event_id: &str,
        user_id: &str,
        email: &str,
        occurred_at: &str,
        source: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO wallet_provisioning_events (event_id, user_id, email, occurred_at, source) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(event_id)
        .bind(user_id)
        .bind(email)
        .bind(occurred_at)
        .bind(source)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_recent(&self, limit: i64) -> Result<Vec<ProvisioningEventRow>, sqlx::Error> {
        let sql = format!(
            "SELECT {PROV_COLS} FROM wallet_provisioning_events ORDER BY processed_at DESC LIMIT ?"
        );
        let rows: Vec<ProvisioningEventRow> = sqlx::query_as(&sql)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows)
    }
}

