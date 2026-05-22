use bidmart_wallet_service_rust::persistence::repositories::{
    TransactionRepository, WalletRepository,
};
use bidmart_wallet_service_rust::server;
use bidmart_wallet_service_rust::wallet::{Money, TransactionType, Wallet, WalletTransaction};

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
async fn hold_funds_reuses_same_auction_hold_when_replacing_bid() {
    let pool = setup_pool().await;
    let repo = WalletRepository::new(pool.clone());

    let mut wallet = Wallet::new("buyer-1", "BUYER");
    repo.insert(&wallet).await.unwrap();
    wallet.top_up(Money::from_rupiah(300_000)).unwrap();
    repo.update(&wallet).await.unwrap();

    let expires_at = "2099-12-31T23:59:59Z";
    repo.hold_funds(
        wallet.id(),
        "auction-a",
        "bid-a-1",
        Money::from_rupiah(5_000),
        "hold-a-1",
        expires_at,
    )
    .await
    .unwrap();
    repo.hold_funds(
        wallet.id(),
        "auction-b",
        "bid-b-1",
        Money::from_rupiah(10_000),
        "hold-b-1",
        expires_at,
    )
    .await
    .unwrap();

    let replaced = repo
        .hold_funds(
            wallet.id(),
            "auction-a",
            "bid-a-2",
            Money::from_rupiah(290_000),
            "hold-a-2",
            expires_at,
        )
        .await
        .unwrap();
    assert_eq!(replaced.amount, 290_000);

    let found = repo
        .find_by_user_id_and_role("buyer-1", "BUYER")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.active_balance(), Money::zero());
    assert_eq!(found.held_balance(), Money::from_rupiah(300_000));

    let old_a_status: String = sqlx::query_scalar("SELECT status FROM holds WHERE id = $1")
        .bind("hold-a-1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let auction_b_status: String = sqlx::query_scalar("SELECT status FROM holds WHERE id = $1")
        .bind("hold-b-1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(old_a_status, "RELEASED");
    assert_eq!(auction_b_status, "ACTIVE");

    let err = repo
        .hold_funds(
            wallet.id(),
            "auction-a",
            "bid-a-3",
            Money::from_rupiah(290_001),
            "hold-a-3",
            expires_at,
        )
        .await
        .unwrap_err();
    assert!(err.contains("insufficient active balance"));
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
    repo.insert(&tx).await.unwrap();

    let history = repo
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap();
    let tx_id = history[0].id.to_string();

    let found = repo.find_by_id(&tx_id).await.unwrap();
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.user_id.as_ref(), "user-1");
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
