use bidmart_wallet_service_rust::service::wallet_service::WalletService;
use bidmart_wallet_service_rust::wallet::Money;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

async fn setup_service() -> WalletService {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();

    let sql = include_str!("../migrations/20260429000000_init.sql");
    for statement in sql.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() {
            sqlx::query(trimmed).execute(&pool).await.unwrap();
        }
    }

    WalletService::new(pool)
}

// ── Create and find ──────────────────────────────────────────────

#[tokio::test]
async fn create_and_find_wallet() {
    let svc = setup_service().await;

    let wallet = svc.create_wallet("user-1").await.unwrap();
    assert_eq!(wallet.user_id(), "user-1");
    assert_eq!(wallet.active_balance(), Money::zero());

    let found = svc.find_by_user_id("user-1").await.unwrap();
    assert_eq!(found.user_id(), "user-1");
}

#[tokio::test]
async fn find_nonexistent_wallet_returns_error() {
    let svc = setup_service().await;
    let result = svc.find_by_user_id("ghost").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn find_all_wallets() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();
    svc.create_wallet("user-2").await.unwrap();

    let all = svc.find_all().await.unwrap();
    assert_eq!(all.len(), 2);
}

// ── Top-up ───────────────────────────────────────────────────────

#[tokio::test]
async fn top_up_increases_active_balance() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();

    let wallet = svc.top_up("user-1", Money::from_cents(5000)).await.unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(5000));

    // Persisted correctly
    let found = svc.find_by_user_id("user-1").await.unwrap();
    assert_eq!(found.active_balance(), Money::from_cents(5000));
}

#[tokio::test]
async fn top_up_zero_fails() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();

    let result = svc.top_up("user-1", Money::zero()).await;
    assert!(result.is_err());
}

// ── Withdraw ─────────────────────────────────────────────────────

#[tokio::test]
async fn withdraw_decreases_active_balance() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();
    svc.top_up("user-1", Money::from_cents(10000)).await.unwrap();

    let wallet = svc.withdraw("user-1", Money::from_cents(3000)).await.unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(7000));
}

#[tokio::test]
async fn withdraw_insufficient_balance_fails() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();

    let result = svc.withdraw("user-1", Money::from_cents(1000)).await;
    assert!(result.is_err());
}

// ── Hold ─────────────────────────────────────────────────────────

#[tokio::test]
async fn hold_moves_active_to_held() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();
    svc.top_up("user-1", Money::from_cents(10000)).await.unwrap();

    let wallet = svc.hold("user-1", Money::from_cents(4000)).await.unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(6000));
    assert_eq!(wallet.held_balance(), Money::from_cents(4000));
}

// ── Release ──────────────────────────────────────────────────────

#[tokio::test]
async fn release_moves_held_to_active() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();
    svc.top_up("user-1", Money::from_cents(10000)).await.unwrap();
    svc.hold("user-1", Money::from_cents(5000)).await.unwrap();

    let wallet = svc.release("user-1", Money::from_cents(3000)).await.unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(8000));
    assert_eq!(wallet.held_balance(), Money::from_cents(2000));
}

// ── Convert ──────────────────────────────────────────────────────

#[tokio::test]
async fn convert_removes_held_balance() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();
    svc.top_up("user-1", Money::from_cents(10000)).await.unwrap();
    svc.hold("user-1", Money::from_cents(5000)).await.unwrap();

    let wallet = svc.convert("user-1", Money::from_cents(5000)).await.unwrap();
    assert_eq!(wallet.held_balance(), Money::zero());
    assert_eq!(wallet.active_balance(), Money::from_cents(5000));
}

// ── Bid ──────────────────────────────────────────────────────────

#[tokio::test]
async fn bid_moves_active_to_held() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();
    svc.top_up("user-1", Money::from_cents(10000)).await.unwrap();

    let wallet = svc.bid("user-1", Money::from_cents(4000)).await.unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(6000));
    assert_eq!(wallet.held_balance(), Money::from_cents(4000));
}

// ── Transaction history ──────────────────────────────────────────

#[tokio::test]
async fn transaction_history_records_all_operations() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();
    svc.top_up("user-1", Money::from_cents(10000)).await.unwrap();
    svc.hold("user-1", Money::from_cents(3000)).await.unwrap();
    svc.release("user-1", Money::from_cents(1000)).await.unwrap();

    let history = svc.get_transaction_history("user-1").await.unwrap();
    assert_eq!(history.len(), 3);
}

// ── Cancel bid ───────────────────────────────────────────────────

#[tokio::test]
async fn cancel_bid_releases_held_funds() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();
    svc.top_up("user-1", Money::from_cents(10000)).await.unwrap();

    let wallet_after_bid = svc.bid("user-1", Money::from_cents(4000)).await.unwrap();
    assert_eq!(wallet_after_bid.held_balance(), Money::from_cents(4000));

    // Get the bid transaction from history to find its ID
    let history = svc.get_transaction_history("user-1").await.unwrap();
    let bid_tx = history.iter().find(|tx| tx.transaction_type == bidmart_wallet_service_rust::wallet::TransactionType::Bid).unwrap();

    svc.cancel_bid("user-1", &bid_tx.id).await.unwrap();

    let wallet = svc.find_by_user_id("user-1").await.unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(10000));
    assert_eq!(wallet.held_balance(), Money::zero());
}

#[tokio::test]
async fn cancel_bid_wrong_user_fails() {
    let svc = setup_service().await;
    svc.create_wallet("user-1").await.unwrap();
    svc.create_wallet("user-2").await.unwrap();
    svc.top_up("user-1", Money::from_cents(10000)).await.unwrap();
    svc.bid("user-1", Money::from_cents(4000)).await.unwrap();

    let history = svc.get_transaction_history("user-1").await.unwrap();
    let bid_tx = history.iter().find(|tx| tx.transaction_type == bidmart_wallet_service_rust::wallet::TransactionType::Bid).unwrap();

    let result = svc.cancel_bid("user-2", &bid_tx.id).await;
    assert!(result.is_err());
}