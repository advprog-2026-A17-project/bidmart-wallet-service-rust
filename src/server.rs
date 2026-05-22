use axum::Router;
use sqlx::AnyPool;
use sqlx::any::{AnyPoolOptions, install_default_drivers};

use crate::http::router::create_router;
use crate::service::wallet_service::WalletService;

pub fn default_database_url() -> String {
    "postgresql://postgres:postgres@localhost:5432/bidmart_wallet_db".to_string()
}

pub fn build_router(pool: AnyPool) -> Router {
    let service = WalletService::new(pool);
    create_router(service)
}

pub async fn connect_pool(database_url: &str) -> Result<AnyPool, sqlx::Error> {
    install_default_drivers();
    let max_connections = if database_url == "sqlite::memory:" {
        1
    } else {
        5
    };

    AnyPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await
}

pub async fn run_migrations(pool: &AnyPool) -> Result<(), sqlx::Error> {
    let sql = include_str!("../migrations/20260429000000_init.sql");
    for statement in sql.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() {
            sqlx::query(trimmed).execute(pool).await?;
        }
    }
    ensure_role_columns(pool).await;
    ensure_rupiah_columns(pool).await;
    ensure_non_negative_constraints(pool).await;
    ensure_payment_intent_columns(pool).await;
    ensure_withdrawal_columns(pool).await;
    Ok(())
}

async fn ensure_role_columns(pool: &AnyPool) {
    let tables = [
        "wallets",
        "wallet_transactions",
        "wallet_payment_intents",
        "wallet_withdrawals",
    ];

    for table in tables {
        let sql = format!("ALTER TABLE {table} ADD COLUMN role TEXT NOT NULL DEFAULT 'BUYER'");
        let _ = sqlx::query(&sql).execute(pool).await;
    }
}

async fn ensure_payment_intent_columns(pool: &AnyPool) {
    let statements = [
        "ALTER TABLE wallet_payment_intents ADD COLUMN va_number TEXT",
        "ALTER TABLE wallet_payment_intents ADD COLUMN payment_channel TEXT",
    ];

    for statement in statements {
        let _ = sqlx::query(statement).execute(pool).await;
    }
}

async fn ensure_rupiah_columns(pool: &AnyPool) {
    let statements = [
        "ALTER TABLE wallets RENAME COLUMN active_balance_cents TO active_balance",
        "ALTER TABLE wallets RENAME COLUMN held_balance_cents TO held_balance",
        "ALTER TABLE wallet_transactions RENAME COLUMN amount_cents TO amount",
        "ALTER TABLE wallet_payment_intents RENAME COLUMN amount_cents TO amount",
        "ALTER TABLE wallet_withdrawals RENAME COLUMN amount_cents TO amount",
    ];

    for statement in statements {
        let _ = sqlx::query(statement).execute(pool).await;
    }
}

async fn ensure_non_negative_constraints(pool: &AnyPool) {
    let statements = [
        "ALTER TABLE wallets ADD CONSTRAINT wallets_active_balance_non_negative CHECK (active_balance >= 0)",
        "ALTER TABLE wallets ADD CONSTRAINT wallets_held_balance_non_negative CHECK (held_balance >= 0)",
        "ALTER TABLE wallet_transactions ADD CONSTRAINT wallet_transactions_amount_non_negative CHECK (amount >= 0)",
        "ALTER TABLE holds ADD CONSTRAINT holds_amount_non_negative CHECK (amount >= 0)",
        "ALTER TABLE wallet_payment_intents ADD CONSTRAINT wallet_payment_intents_amount_non_negative CHECK (amount >= 0)",
        "ALTER TABLE wallet_withdrawals ADD CONSTRAINT wallet_withdrawals_amount_non_negative CHECK (amount >= 0)",
    ];

    for statement in statements {
        let _ = sqlx::query(statement).execute(pool).await;
    }
}

async fn ensure_withdrawal_columns(pool: &AnyPool) {
    let statements = [
        "ALTER TABLE wallet_withdrawals ADD COLUMN bank_code TEXT",
        "ALTER TABLE wallet_withdrawals ADD COLUMN account_number TEXT",
        "ALTER TABLE wallet_withdrawals ADD COLUMN account_name TEXT",
        "ALTER TABLE wallet_withdrawals ADD COLUMN payout_reference TEXT",
        "ALTER TABLE wallet_withdrawals ADD COLUMN failure_reason TEXT",
    ];

    for statement in statements {
        let _ = sqlx::query(statement).execute(pool).await;
    }
}
