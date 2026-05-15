use sqlx::{Any, AnyPool, Transaction};

use crate::persistence::models::{
    HoldRow, PaymentIntentRow, ProvisioningEventRow, TransactionRow, WalletRow, WithdrawalRow,
};
use crate::wallet::{
    Hold, HoldStatus, Money, PaymentIntent, TransactionType, Wallet, WalletTransaction,
    WalletWithdrawal,
};

use chrono::{DateTime, NaiveDateTime, Utc};

// ── Column lists (DRY) ──────────────────────────────────────────

const WALLET_COLS: &str = "id, user_id, active_balance_cents, held_balance_cents, version";
const TX_COLS: &str =
    "id, user_id, transaction_type, amount_cents, created_at, correlation_id, source_service";
const PROV_COLS: &str = "event_id, user_id, email, occurred_at, source, processed_at";
const HOLD_COLS: &str =
    "id, wallet_id, auction_id, bid_id, amount, status, expires_at, created_at, updated_at";
const PAYMENT_COLS: &str =
    "id, user_id, amount_cents, status, redirect_url, va_number, payment_channel, created_at, updated_at";
const WITHDRAWAL_COLS: &str =
    "id, user_id, amount_cents, bank_account, status, created_at, updated_at";

// ── Row → Domain mappers ────────────────────────────────────────

fn wallet_from_row(row: WalletRow) -> Wallet {
    Wallet::with_balances(
        row.id,
        row.user_id,
        Money::from_cents(row.active_balance_cents as u64),
        Money::from_cents(row.held_balance_cents as u64),
        row.version,
    )
}

fn transaction_from_row(row: TransactionRow) -> WalletTransaction {
    let ts = normalize_timestamp(row.created_at);
    WalletTransaction {
        id: row.id,
        user_id: row.user_id,
        transaction_type: TransactionType::from_str(&row.transaction_type),
        amount: Money::from_cents(row.amount_cents as u64),
        created_at: Some(ts),
        correlation_id: row.correlation_id,
        source_service: row.source_service,
    }
}

fn normalize_timestamp(input: String) -> String {
    // Try RFC3339 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(&input) {
        return dt.with_timezone(&Utc).to_rfc3339();
    }

    // Fallback: common SQL datetime format `YYYY-MM-DD HH:MM:SS`
    if let Ok(naive) = NaiveDateTime::parse_from_str(&input, "%Y-%m-%d %H:%M:%S") {
        return DateTime::<Utc>::from_utc(naive, Utc).to_rfc3339();
    }

    // Unknown format: return original string
    input
}

// ── WalletRepository ────────────────────────────────────────────

pub struct WalletRepository {
    pool: AnyPool,
}

impl WalletRepository {
    pub fn new(pool: AnyPool) -> Self {
        Self { pool }
    }

    pub async fn begin_tx(&self) -> Result<Transaction<'_, Any>, sqlx::Error> {
        self.pool.begin().await
    }

    pub async fn insert(&self, wallet: &Wallet) -> Result<(), sqlx::Error> {
        let sql = format!("INSERT INTO wallets ({WALLET_COLS}) VALUES ($1, $2, $3, $4, $5)");
        sqlx::query(&sql)
            .bind(wallet.id())
            .bind(wallet.user_id())
            .bind(wallet.active_balance().cents() as i64)
            .bind(wallet.held_balance().cents() as i64)
            .bind(wallet.version())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn find_by_user_id(&self, user_id: &str) -> Result<Option<Wallet>, sqlx::Error> {
        let sql = format!("SELECT {WALLET_COLS} FROM wallets WHERE user_id = $1");
        let row: Option<WalletRow> = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(wallet_from_row))
    }

    pub async fn find_by_user_id_tx(
        &self,
        tx: &mut Transaction<'_, Any>,
        user_id: &str,
    ) -> Result<Option<Wallet>, sqlx::Error> {
        let sql = format!("SELECT {WALLET_COLS} FROM wallets WHERE user_id = $1");
        let row: Option<WalletRow> = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await?;

        Ok(row.map(wallet_from_row))
    }

    pub async fn update(&self, wallet: &Wallet) -> Result<(), sqlx::Error> {
        let result = sqlx::query(
            "UPDATE wallets SET active_balance_cents = $1, held_balance_cents = $2, version = version + 1 WHERE id = $3 AND version = $4",
        )
        .bind(wallet.active_balance().cents() as i64)
        .bind(wallet.held_balance().cents() as i64)
        .bind(wallet.id())
        .bind(wallet.version())
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }
        Ok(())
    }

    pub async fn update_tx(
        &self,
        tx: &mut Transaction<'_, Any>,
        wallet: &Wallet,
    ) -> Result<(), sqlx::Error> {
        let result = sqlx::query(
            "UPDATE wallets SET active_balance_cents = $1, held_balance_cents = $2, version = version + 1 WHERE id = $3 AND version = $4",
        )
        .bind(wallet.active_balance().cents() as i64)
        .bind(wallet.held_balance().cents() as i64)
        .bind(wallet.id())
        .bind(wallet.version())
        .execute(&mut **tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }
        Ok(())
    }

    pub async fn find_all(&self) -> Result<Vec<Wallet>, sqlx::Error> {
        let sql = format!("SELECT {WALLET_COLS} FROM wallets");
        let rows: Vec<WalletRow> = sqlx::query_as(&sql).fetch_all(&self.pool).await?;

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
        let existing_hold_sql =
            format!("SELECT {HOLD_COLS} FROM holds WHERE bid_id = $1 AND auction_id = $2");
        let existing_hold: Option<HoldRow> = sqlx::query_as(&existing_hold_sql)
            .bind(bid_id)
            .bind(auction_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(row) = existing_hold {
            return Hold::try_from(row);
        }

        let wallet_sql = format!("SELECT {WALLET_COLS} FROM wallets WHERE id = $1");
        let wallet_row: Option<WalletRow> = sqlx::query_as(&wallet_sql)
            .bind(wallet_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let mut wallet = wallet_row.map(wallet_from_row).ok_or("Wallet not found")?;

        let wallet_tx = wallet.hold(amount).map_err(|e| e.to_string())?;

        let result = sqlx::query("UPDATE wallets SET active_balance_cents = $1, held_balance_cents = $2, version = version + 1 WHERE id = $3 AND version = $4")
            .bind(wallet.active_balance().cents() as i64)
            .bind(wallet.held_balance().cents() as i64)
            .bind(wallet.id())
            .bind(wallet.version())
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        if result.rows_affected() == 0 {
            return Err("CONCURRENCY_CONFLICT: Wallet is being modified by another operation. Please try again.".to_string());
        }

        sqlx::query("INSERT INTO wallet_transactions (id, user_id, transaction_type, amount_cents, correlation_id, source_service) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(&wallet_tx.id)
            .bind(&wallet_tx.user_id)
            .bind(wallet_tx.transaction_type.as_str())
            .bind(wallet_tx.amount.cents() as i64)
            .bind(&wallet_tx.correlation_id)
            .bind(&wallet_tx.source_service)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let status_str = HoldStatus::Active.to_string();
        sqlx::query("INSERT INTO holds (id, wallet_id, auction_id, bid_id, amount, status, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7)")
            .bind(hold_id)
            .bind(wallet_id)
            .bind(auction_id)
            .bind(bid_id)
            .bind(amount.cents() as i64)
            .bind(&status_str)
            .bind(expires_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        let new_hold_sql = format!("SELECT {HOLD_COLS} FROM holds WHERE id = $1");
        let new_hold_row: HoldRow = sqlx::query_as(&new_hold_sql)
            .bind(hold_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| e.to_string())?;

        Hold::try_from(new_hold_row)
    }

    pub async fn release_funds(&self, hold_id: &str) -> Result<Hold, String> {
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        let hold_sql = format!("SELECT {HOLD_COLS} FROM holds WHERE id = $1");
        let hold_row: Option<HoldRow> = sqlx::query_as(&hold_sql)
            .bind(hold_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let hold_row = hold_row.ok_or("Hold record not found")?;
        let mut hold = Hold::try_from(hold_row)?;

        if hold.status != HoldStatus::Active {
            return Ok(hold);
        }

        let wallet_sql = format!("SELECT {WALLET_COLS} FROM wallets WHERE id = $1");
        let wallet_row: Option<WalletRow> = sqlx::query_as(&wallet_sql)
            .bind(&hold.wallet_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let mut wallet = wallet_row.map(wallet_from_row).ok_or("Wallet not found")?;

        let amount_money = Money::from_cents(hold.amount as u64);
        let wallet_tx = wallet.release(amount_money).map_err(|e| e.to_string())?;

        let result = sqlx::query("UPDATE wallets SET active_balance_cents = $1, held_balance_cents = $2, version = version + 1 WHERE id = $3 AND version = $4")
            .bind(wallet.active_balance().cents() as i64)
            .bind(wallet.held_balance().cents() as i64)
            .bind(wallet.id())
            .bind(wallet.version())
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        if result.rows_affected() == 0 {
            return Err("CONCURRENCY_CONFLICT: Wallet is being modified by another operation. Please try again.".to_string());
        }

        sqlx::query("INSERT INTO wallet_transactions (id, user_id, transaction_type, amount_cents, correlation_id, source_service) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(&wallet_tx.id)
            .bind(&wallet_tx.user_id)
            .bind(wallet_tx.transaction_type.as_str())
            .bind(wallet_tx.amount.cents() as i64)
            .bind(&wallet_tx.correlation_id)
            .bind(&wallet_tx.source_service)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let released_status = HoldStatus::Released.to_string();
        let updated_at = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE holds SET status = $1, updated_at = $2 WHERE id = $3")
            .bind(&released_status)
            .bind(updated_at)
            .bind(hold_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        hold.status = HoldStatus::Released;
        Ok(hold)
    }

    pub async fn convert_funds(&self, hold_id: &str) -> Result<Hold, String> {
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        let hold_sql = format!("SELECT {HOLD_COLS} FROM holds WHERE id = $1");
        let hold_row: Option<HoldRow> = sqlx::query_as(&hold_sql)
            .bind(hold_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let hold_row = hold_row.ok_or("Hold record not found")?;
        let mut hold = Hold::try_from(hold_row)?;

        if hold.status != HoldStatus::Active {
            return Ok(hold);
        }

        let wallet_sql = format!("SELECT {WALLET_COLS} FROM wallets WHERE id = $1");
        let wallet_row: Option<WalletRow> = sqlx::query_as(&wallet_sql)
            .bind(&hold.wallet_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let mut wallet = wallet_row.map(wallet_from_row).ok_or("Wallet not found")?;

        let amount_money = Money::from_cents(hold.amount as u64);
        let wallet_tx = wallet.convert(amount_money).map_err(|e| e.to_string())?;

        let result = sqlx::query("UPDATE wallets SET active_balance_cents = $1, held_balance_cents = $2, version = version + 1 WHERE id = $3 AND version = $4")
            .bind(wallet.active_balance().cents() as i64)
            .bind(wallet.held_balance().cents() as i64)
            .bind(wallet.id())
            .bind(wallet.version())
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        if result.rows_affected() == 0 {
            return Err("CONCURRENCY_CONFLICT: Wallet is being modified by another operation. Please try again.".to_string());
        }

        sqlx::query("INSERT INTO wallet_transactions (id, user_id, transaction_type, amount_cents, correlation_id, source_service) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(&wallet_tx.id)
            .bind(&wallet_tx.user_id)
            .bind(wallet_tx.transaction_type.as_str())
            .bind(wallet_tx.amount.cents() as i64)
            .bind(&wallet_tx.correlation_id)
            .bind(&wallet_tx.source_service)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let converted_status = HoldStatus::Converted.to_string();
        let updated_at = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE holds SET status = $1, updated_at = $2 WHERE id = $3")
            .bind(&converted_status)
            .bind(updated_at)
            .bind(hold_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        hold.status = HoldStatus::Converted;
        Ok(hold)
    }

    pub async fn find_expired_holds(&self) -> Result<Vec<String>, sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT id FROM holds WHERE status = 'ACTIVE' AND expires_at < $1")
                .bind(now)
                .fetch_all(&self.pool)
                .await?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    pub async fn insert_payment_intent(
        &self,
        payment_id: &str,
        user_id: &str,
        amount: Money,
        redirect_url: &str,
        va_number: Option<&str>,
        payment_channel: Option<&str>,
    ) -> Result<PaymentIntent, sqlx::Error> {
        sqlx::query(
            "INSERT INTO wallet_payment_intents (id, user_id, amount_cents, status, redirect_url, va_number, payment_channel) VALUES ($1, $2, $3, 'PENDING', $4, $5, $6)",
        )
        .bind(payment_id)
        .bind(user_id)
        .bind(amount.cents() as i64)
        .bind(redirect_url)
        .bind(va_number)
        .bind(payment_channel)
        .execute(&self.pool)
        .await?;

        self.find_payment_intent(payment_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn find_payment_intent(
        &self,
        payment_id: &str,
    ) -> Result<Option<PaymentIntent>, sqlx::Error> {
        let sql = format!("SELECT {PAYMENT_COLS} FROM wallet_payment_intents WHERE id = $1");
        let row: Option<PaymentIntentRow> = sqlx::query_as(&sql)
            .bind(payment_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(PaymentIntent::from))
    }

    pub async fn find_payment_intent_tx(
        &self,
        tx: &mut Transaction<'_, Any>,
        payment_id: &str,
    ) -> Result<Option<PaymentIntent>, sqlx::Error> {
        let sql = format!("SELECT {PAYMENT_COLS} FROM wallet_payment_intents WHERE id = $1");
        let row: Option<PaymentIntentRow> = sqlx::query_as(&sql)
            .bind(payment_id)
            .fetch_optional(&mut **tx)
            .await?;

        Ok(row.map(PaymentIntent::from))
    }

    pub async fn update_payment_status(
        &self,
        payment_id: &str,
        status: &str,
    ) -> Result<(), sqlx::Error> {
        let updated_at = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE wallet_payment_intents SET status = $1, updated_at = $2 WHERE id = $3")
            .bind(status)
            .bind(updated_at)
            .bind(payment_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_payment_status_if_pending_tx(
        &self,
        tx: &mut Transaction<'_, Any>,
        payment_id: &str,
        status: &str,
    ) -> Result<bool, sqlx::Error> {
        let updated_at = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE wallet_payment_intents SET status = $1, updated_at = $2 WHERE id = $3 AND status = 'PENDING'",
        )
        .bind(status)
        .bind(updated_at)
        .bind(payment_id)
        .execute(&mut **tx)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn insert_withdrawal(
        &self,
        user_id: &str,
        amount: Money,
        bank_account: &str,
    ) -> Result<WalletWithdrawal, sqlx::Error> {
        let withdrawal_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO wallet_withdrawals (id, user_id, amount_cents, bank_account, status) VALUES ($1, $2, $3, $4, 'PENDING')",
        )
        .bind(&withdrawal_id)
        .bind(user_id)
        .bind(amount.cents() as i64)
        .bind(bank_account)
        .execute(&self.pool)
        .await?;

        self.find_withdrawal(&withdrawal_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn find_withdrawal(
        &self,
        withdrawal_id: &str,
    ) -> Result<Option<WalletWithdrawal>, sqlx::Error> {
        let sql = format!("SELECT {WITHDRAWAL_COLS} FROM wallet_withdrawals WHERE id = $1");
        let row: Option<WithdrawalRow> = sqlx::query_as(&sql)
            .bind(withdrawal_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(WalletWithdrawal::from))
    }

    pub async fn update_withdrawal_status(
        &self,
        withdrawal_id: &str,
        status: &str,
    ) -> Result<(), sqlx::Error> {
        let updated_at = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE wallet_withdrawals SET status = $1, updated_at = $2 WHERE id = $3")
            .bind(status)
            .bind(updated_at)
            .bind(withdrawal_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

// ── TransactionRepository ───────────────────────────────────────

pub struct TransactionRepository {
    pool: AnyPool,
}

impl TransactionRepository {
    pub fn new(pool: AnyPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, tx: &WalletTransaction) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO wallet_transactions (id, user_id, transaction_type, amount_cents, correlation_id, source_service) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&tx.id)
        .bind(&tx.user_id)
        .bind(tx.transaction_type.as_str())
        .bind(tx.amount.cents() as i64)
        .bind(&tx.correlation_id)
        .bind(&tx.source_service)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_tx(
        &self,
        db_tx: &mut Transaction<'_, Any>,
        tx: &WalletTransaction,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO wallet_transactions (id, user_id, transaction_type, amount_cents, correlation_id, source_service) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&tx.id)
        .bind(&tx.user_id)
        .bind(tx.transaction_type.as_str())
        .bind(tx.amount.cents() as i64)
        .bind(&tx.correlation_id)
        .bind(&tx.source_service)
        .execute(&mut **db_tx)
        .await?;
        Ok(())
    }

    pub async fn find_by_user_id(
        &self,
        user_id: &str,
    ) -> Result<Vec<WalletTransaction>, sqlx::Error> {
        let sql = format!(
            "SELECT {TX_COLS} FROM wallet_transactions WHERE user_id = $1 ORDER BY created_at DESC, id DESC"
        );
        let rows: Vec<TransactionRow> = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(transaction_from_row).collect())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<WalletTransaction>, sqlx::Error> {
        let sql = format!("SELECT {TX_COLS} FROM wallet_transactions WHERE id = $1");
        let row: Option<TransactionRow> = sqlx::query_as(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(transaction_from_row))
    }
}

// ── ProvisioningEventRepository ─────────────────────────────────

pub struct ProvisioningEventRepository {
    pool: AnyPool,
}

impl ProvisioningEventRepository {
    pub fn new(pool: AnyPool) -> Self {
        Self { pool }
    }

    pub async fn exists(&self, event_id: &str) -> Result<bool, sqlx::Error> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM wallet_provisioning_events WHERE event_id = $1")
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
            "INSERT INTO wallet_provisioning_events (event_id, user_id, email, occurred_at, source) VALUES ($1, $2, $3, $4, $5)",
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
            "SELECT {PROV_COLS} FROM wallet_provisioning_events ORDER BY processed_at DESC LIMIT $1"
        );
        let rows: Vec<ProvisioningEventRow> = sqlx::query_as(&sql)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows)
    }
}
