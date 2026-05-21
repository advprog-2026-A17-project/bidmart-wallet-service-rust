use bidmart_wallet_service_rust::persistence::repositories::{
    TransactionRepository, WalletRepository,
};
use bidmart_wallet_service_rust::server;
use bidmart_wallet_service_rust::wallet::{
    HoldStatus, Money, TransactionType, Wallet, WalletTransaction,
};

use sqlx::AnyPool;

async fn setup_pool() -> AnyPool {
    let pool = server::connect_pool("sqlite::memory:").await.unwrap();
    server::run_migrations(&pool).await.unwrap();
    pool
}

// ── WalletRepository tests ──────────────────────────────────────

#[tokio::test]
async fn insert_and_find_wallet_by_user_id() {
    let pool = setup_pool().await;
    let repo = WalletRepository::new(pool);

    let wallet = Wallet::new("user-1", "BUYER");
    repo.insert(&wallet).await.unwrap();

    let found = repo
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap();
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.user_id(), "user-1");
    assert_eq!(found.active_balance(), Money::zero());
    assert_eq!(found.held_balance(), Money::zero());
}

#[tokio::test]
async fn find_by_user_id_returns_none_when_absent() {
    let pool = setup_pool().await;
    let repo = WalletRepository::new(pool);

    let found = repo
        .find_by_user_id_and_role("nonexistent", "BUYER")
        .await
        .unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn update_wallet_balances() {
    let pool = setup_pool().await;
    let repo = WalletRepository::new(pool);

    let mut wallet = Wallet::new("user-1", "BUYER");
    repo.insert(&wallet).await.unwrap();

    wallet.top_up(Money::from_cents(10000)).unwrap();
    wallet.hold(Money::from_cents(3000)).unwrap();
    repo.update(&wallet).await.unwrap();

    let found = repo
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.active_balance(), Money::from_cents(7000));
    assert_eq!(found.held_balance(), Money::from_cents(3000));
}

#[tokio::test]
async fn find_all_wallets() {
    let pool = setup_pool().await;
    let repo = WalletRepository::new(pool);

    repo.insert(&Wallet::new("user-1", "BUYER")).await.unwrap();
    repo.insert(&Wallet::new("user-2", "BUYER")).await.unwrap();

    let all = repo.find_all().await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn duplicate_user_id_insert_fails() {
    let pool = setup_pool().await;
    let repo = WalletRepository::new(pool);

    repo.insert(&Wallet::new("user-1", "BUYER")).await.unwrap();
    let result = repo.insert(&Wallet::new("user-1", "BUYER")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn stale_wallet_update_fails_optimistic_lock() {
    let pool = setup_pool().await;
    let repo = WalletRepository::new(pool);

    let mut stale = Wallet::new("user-1", "BUYER");
    repo.insert(&stale).await.unwrap();

    let mut fresh = repo
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap()
        .unwrap();
    fresh.top_up(Money::from_cents(1000)).unwrap();
    repo.update(&fresh).await.unwrap();

    stale.top_up(Money::from_cents(500)).unwrap();
    let result = repo.update(&stale).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn hold_release_convert_repository_flow_is_idempotent() {
    let pool = setup_pool().await;
    let repo = WalletRepository::new(pool);

    let mut wallet = Wallet::new("user-1", "BUYER");
    wallet.top_up(Money::from_cents(10_000)).unwrap();
    repo.insert(&wallet).await.unwrap();
    repo.update(&wallet).await.unwrap();

    let hold = repo
        .hold_funds(
            wallet.id(),
            "auc-1",
            "bid-1",
            Money::from_cents(4000),
            "hold-1",
            "2026-12-31T23:59:59Z",
        )
        .await
        .unwrap();
    let duplicate = repo
        .hold_funds(
            wallet.id(),
            "auc-1",
            "bid-1",
            Money::from_cents(4000),
            "hold-duplicate",
            "2026-12-31T23:59:59Z",
        )
        .await
        .unwrap();

    assert_eq!(hold.id, "hold-1");
    assert_eq!(duplicate.id, "hold-1");
    assert_eq!(duplicate.status, HoldStatus::Active);

    let released = repo.release_funds("hold-1").await.unwrap();
    let released_again = repo.release_funds("hold-1").await.unwrap();
    assert_eq!(released.status, HoldStatus::Released);
    assert_eq!(released_again.status, HoldStatus::Released);

    let active = repo
        .hold_funds(
            wallet.id(),
            "auc-2",
            "bid-2",
            Money::from_cents(3000),
            "hold-2",
            "2026-12-31T23:59:59Z",
        )
        .await
        .unwrap();
    assert_eq!(active.status, HoldStatus::Active);
    let converted = repo.convert_funds("hold-2").await.unwrap();
    let converted_again = repo.convert_funds("hold-2").await.unwrap();
    assert_eq!(converted.status, HoldStatus::Converted);
    assert_eq!(converted_again.status, HoldStatus::Converted);
}

#[tokio::test]
async fn find_expired_holds_only_returns_active_expired_holds() {
    let pool = setup_pool().await;
    let repo = WalletRepository::new(pool);

    let mut wallet = Wallet::new("user-1", "BUYER");
    wallet.top_up(Money::from_cents(10_000)).unwrap();
    repo.insert(&wallet).await.unwrap();
    repo.update(&wallet).await.unwrap();

    repo.hold_funds(
        wallet.id(),
        "auc-old",
        "bid-old",
        Money::from_cents(1000),
        "hold-old",
        "2000-01-01T00:00:00Z",
    )
    .await
    .unwrap();
    repo.hold_funds(
        wallet.id(),
        "auc-future",
        "bid-future",
        Money::from_cents(1000),
        "hold-future",
        "2999-01-01T00:00:00Z",
    )
    .await
    .unwrap();
    repo.release_funds("hold-old").await.unwrap();

    let expired = repo.find_expired_holds().await.unwrap();

    assert!(expired.is_empty());
}

#[tokio::test]
async fn payment_intent_and_withdrawal_repository_roundtrip() {
    let pool = setup_pool().await;
    let repo = WalletRepository::new(pool);

    let payment = repo
        .insert_payment_intent(
            "pay-1",
            "user-1",
            "BUYER",
            Money::from_cents(5000),
            "http://local/pay-1",
            Some("123456"),
            Some("local-bca_va"),
        )
        .await
        .unwrap();
    assert_eq!(payment.status, "PENDING");

    repo.update_payment_status("pay-1", "FAILED").await.unwrap();
    let failed = repo.find_payment_intent("pay-1").await.unwrap().unwrap();
    assert_eq!(failed.status, "FAILED");
    let unpaid = repo
        .find_unpaid_payment_intents_by_user("user-1")
        .await
        .unwrap();
    assert_eq!(unpaid.len(), 1);

    let withdrawal = repo
        .insert_withdrawal(
            "user-1",
            "BUYER",
            Money::from_cents(3000),
            "bca",
            "1234567890",
            "Validated Account",
            "WD-123",
        )
        .await
        .unwrap();
    assert_eq!(withdrawal.status, "PENDING");

    repo.update_withdrawal_status(&withdrawal.id, "COMPLETED")
        .await
        .unwrap();
    let completed = repo.find_withdrawal(&withdrawal.id).await.unwrap().unwrap();
    assert_eq!(completed.status, "COMPLETED");
}

// ── TransactionRepository tests ─────────────────────────────────

#[tokio::test]
async fn insert_and_find_transactions_by_user_id() {
    let pool = setup_pool().await;
    let repo = TransactionRepository::new(pool);

    let tx1 = WalletTransaction::new(
        "user-1",
        "BUYER",
        TransactionType::TopUp,
        Money::from_cents(5000),
    );
    let tx2 = WalletTransaction::new(
        "user-1",
        "BUYER",
        TransactionType::Hold,
        Money::from_cents(2000),
    );
    let tx3 = WalletTransaction::new(
        "user-2",
        "BUYER",
        TransactionType::TopUp,
        Money::from_cents(9000),
    );

    repo.insert(&tx1).await.unwrap();
    repo.insert(&tx2).await.unwrap();
    repo.insert(&tx3).await.unwrap();

    let history = repo
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    assert!(
        history
            .iter()
            .any(|tx| tx.transaction_type == TransactionType::Hold)
    );
    assert!(
        history
            .iter()
            .any(|tx| tx.transaction_type == TransactionType::TopUp)
    );
}

#[tokio::test]
async fn find_transaction_by_id() {
    let pool = setup_pool().await;
    let repo = TransactionRepository::new(pool);

    let tx = WalletTransaction::new(
        "user-1",
        "BUYER",
        TransactionType::Bid,
        Money::from_cents(3000),
    );
    let tx_id = tx.id.clone();
    repo.insert(&tx).await.unwrap();

    let found = repo.find_by_id(&tx_id).await.unwrap();
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.user_id, "user-1");
    assert_eq!(found.transaction_type, TransactionType::Bid);
    assert_eq!(found.amount, Money::from_cents(3000));
}

#[tokio::test]
async fn find_transaction_by_id_returns_none_when_absent() {
    let pool = setup_pool().await;
    let repo = TransactionRepository::new(pool);

    let found = repo.find_by_id("nonexistent").await.unwrap();
    assert!(found.is_none());
}
