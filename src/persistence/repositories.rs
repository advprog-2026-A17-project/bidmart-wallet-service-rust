use sqlx::SqlitePool;

use crate::persistence::models::{ProvisioningEventRow, TransactionRow, WalletRow, HoldRow};
use crate::wallet::{Money, TransactionType, Wallet, WalletTransaction, Hold, HoldStatus};

// ── Column lists (DRY) ──────────────────────────────────────────

const WALLET_COLS: &str = "id, user_id, active_balance_cents, held_balance_cents";
const TX_COLS: &str = "id, user_id, transaction_type, amount_cents, created_at";
const PROV_COLS: &str = "event_id, user_id, email, occurred_at, source, processed_at";
const HOLD_COLS: &str = "id, wallet_id, auction_id, bid_id, amount, status, expires_at, created_at, updated_at";

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

    pub async fn hold_funds(
        &self,
        wallet_id: &str,
        auction_id: &str,
        bid_id: &str,
        amount: Money,
        hold_id: &str,
        expires_at: &str,
    ) -> Result<Hold, String> {
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;
        let existing_hold_sql = format!("SELECT {HOLD_COLS} FROM holds WHERE bid_id = ? AND auction_id = ?");
        let existing_hold: Option<HoldRow> = sqlx::query_as(&existing_hold_sql)
            .bind(bid_id)
            .bind(auction_id)
            .fetch_optional(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        if let Some(row) = existing_hold {
            return Hold::try_from(row); 
        }

        let wallet_sql = format!("SELECT {WALLET_COLS} FROM wallets WHERE id = ?");
        let wallet_row: Option<WalletRow> = sqlx::query_as(&wallet_sql)
            .bind(wallet_id)
            .fetch_optional(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        let mut wallet = wallet_row.map(wallet_from_row).ok_or("Wallet not found")?;

        let wallet_tx = wallet.hold(amount).map_err(|e| e.to_string())?;

        sqlx::query("UPDATE wallets SET active_balance_cents = ?, held_balance_cents = ? WHERE id = ?")
            .bind(wallet.active_balance().cents() as i64)
            .bind(wallet.held_balance().cents() as i64)
            .bind(wallet.id())
            .execute(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        sqlx::query("INSERT INTO wallet_transactions (id, user_id, transaction_type, amount_cents) VALUES (?, ?, ?, ?)")
            .bind(&wallet_tx.id)
            .bind(&wallet_tx.user_id)
            .bind(wallet_tx.transaction_type.as_str())
            .bind(wallet_tx.amount.cents() as i64)
            .execute(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        let status_str = HoldStatus::Active.to_string();
        sqlx::query("INSERT INTO holds (id, wallet_id, auction_id, bid_id, amount, status, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(hold_id)
            .bind(wallet_id)
            .bind(auction_id)
            .bind(bid_id)
            .bind(amount.cents() as i64)
            .bind(&status_str)
            .bind(expires_at)
            .execute(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        let new_hold_sql = format!("SELECT {HOLD_COLS} FROM holds WHERE id = ?");
        let new_hold_row: HoldRow = sqlx::query_as(&new_hold_sql)
            .bind(hold_id)
            .fetch_one(&self.pool)
            .await.map_err(|e| e.to_string())?;

        Hold::try_from(new_hold_row)
    }

    pub async fn release_funds(&self, hold_id: &str) -> Result<Hold, String> {
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        let hold_sql = format!("SELECT {HOLD_COLS} FROM holds WHERE id = ?");
        let hold_row: Option<HoldRow> = sqlx::query_as(&hold_sql)
            .bind(hold_id)
            .fetch_optional(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        let hold_row = hold_row.ok_or("Hold record not found")?;
        let mut hold = Hold::try_from(hold_row)?;

        if hold.status != HoldStatus::Active {
            return Ok(hold);
        }

        let wallet_sql = format!("SELECT {WALLET_COLS} FROM wallets WHERE id = ?");
        let wallet_row: Option<WalletRow> = sqlx::query_as(&wallet_sql)
            .bind(&hold.wallet_id)
            .fetch_optional(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        let mut wallet = wallet_row.map(wallet_from_row).ok_or("Wallet not found")?;

        let amount_money = Money::from_cents(hold.amount as u64);
        let wallet_tx = wallet.release(amount_money).map_err(|e| e.to_string())?;

        sqlx::query("UPDATE wallets SET active_balance_cents = ?, held_balance_cents = ? WHERE id = ?")
            .bind(wallet.active_balance().cents() as i64)
            .bind(wallet.held_balance().cents() as i64)
            .bind(wallet.id())
            .execute(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        sqlx::query("INSERT INTO wallet_transactions (id, user_id, transaction_type, amount_cents) VALUES (?, ?, ?, ?)")
            .bind(&wallet_tx.id)
            .bind(&wallet_tx.user_id)
            .bind(wallet_tx.transaction_type.as_str())
            .bind(wallet_tx.amount.cents() as i64)
            .execute(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        let released_status = HoldStatus::Released.to_string();
        sqlx::query("UPDATE holds SET status = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(&released_status)
            .bind(hold_id)
            .execute(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        hold.status = HoldStatus::Released;
        Ok(hold)
    }

    pub async fn convert_funds(&self, hold_id: &str) -> Result<Hold, String> {
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        let hold_sql = format!("SELECT {HOLD_COLS} FROM holds WHERE id = ?");
        let hold_row: Option<HoldRow> = sqlx::query_as(&hold_sql)
            .bind(hold_id)
            .fetch_optional(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        let hold_row = hold_row.ok_or("Hold record not found")?;
        let mut hold = Hold::try_from(hold_row)?;

        if hold.status != HoldStatus::Active {
            return Ok(hold);
        }

        let wallet_sql = format!("SELECT {WALLET_COLS} FROM wallets WHERE id = ?");
        let wallet_row: Option<WalletRow> = sqlx::query_as(&wallet_sql)
            .bind(&hold.wallet_id)
            .fetch_optional(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        let mut wallet = wallet_row.map(wallet_from_row).ok_or("Wallet not found")?;

        let amount_money = Money::from_cents(hold.amount as u64);
        let wallet_tx = wallet.convert(amount_money).map_err(|e| e.to_string())?;

        sqlx::query("UPDATE wallets SET active_balance_cents = ?, held_balance_cents = ? WHERE id = ?")
            .bind(wallet.active_balance().cents() as i64)
            .bind(wallet.held_balance().cents() as i64)
            .bind(wallet.id())
            .execute(&mut *tx)
            .await.map_err(|e| e.to_string())?;
       
        sqlx::query("INSERT INTO wallet_transactions (id, user_id, transaction_type, amount_cents) VALUES (?, ?, ?, ?)")
            .bind(&wallet_tx.id)
            .bind(&wallet_tx.user_id)
            .bind(wallet_tx.transaction_type.as_str())
            .bind(wallet_tx.amount.cents() as i64)
            .execute(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        let converted_status = HoldStatus::Converted.to_string();
        sqlx::query("UPDATE holds SET status = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(&converted_status)
            .bind(hold_id)
            .execute(&mut *tx)
            .await.map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        hold.status = HoldStatus::Converted;
        Ok(hold)
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

