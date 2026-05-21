use bidmart_wallet_service_rust::server;
use bidmart_wallet_service_rust::service::wallet_service::WalletService;
use bidmart_wallet_service_rust::wallet::{Money, TransactionType};

async fn setup_service() -> WalletService {
    let pool = server::connect_pool("sqlite::memory:").await.unwrap();
    server::run_migrations(&pool).await.unwrap();
    WalletService::new(pool)
}

// ── Create and find ──────────────────────────────────────────────

#[tokio::test]
async fn create_and_find_wallet() {
    let svc = setup_service().await;

    let wallet = svc.create_wallet("user-1", "BUYER").await.unwrap();
    assert_eq!(wallet.user_id(), "user-1");
    assert_eq!(wallet.active_balance(), Money::zero());

    let found = svc
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap();
    assert_eq!(found.user_id(), "user-1");
}

#[tokio::test]
async fn find_nonexistent_wallet_returns_error() {
    let svc = setup_service().await;
    let result = svc.find_by_user_id_and_role("ghost", "BUYER").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn find_all_wallets() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.create_wallet("user-2", "BUYER").await.unwrap();

    let all = svc.find_all().await.unwrap();
    assert_eq!(all.len(), 2);
}

// ── Top-up ───────────────────────────────────────────────────────

#[tokio::test]
async fn top_up_increases_active_balance() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();

    let wallet = svc
        .top_up("user-1", "BUYER", Money::from_cents(5000))
        .await
        .unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(5000));

    // Persisted correctly
    let found = svc
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap();
    assert_eq!(found.active_balance(), Money::from_cents(5000));
}

#[tokio::test]
async fn top_up_zero_fails() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();

    let result = svc.top_up("user-1", "BUYER", Money::zero()).await;
    assert!(result.is_err());
}

// ── Withdraw ─────────────────────────────────────────────────────

#[tokio::test]
async fn withdraw_decreases_active_balance() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.top_up("user-1", "BUYER", Money::from_cents(10000))
        .await
        .unwrap();

    let wallet = svc
        .withdraw("user-1", "BUYER", Money::from_cents(3000))
        .await
        .unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(7000));
}

#[tokio::test]
async fn withdraw_insufficient_balance_fails() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();

    let result = svc
        .withdraw("user-1", "BUYER", Money::from_cents(1000))
        .await;
    assert!(result.is_err());
}

// ── Hold ─────────────────────────────────────────────────────────

#[tokio::test]
async fn hold_moves_active_to_held() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.top_up("user-1", "BUYER", Money::from_cents(10000))
        .await
        .unwrap();

    let wallet = svc
        .hold("user-1", "BUYER", Money::from_cents(4000))
        .await
        .unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(6000));
    assert_eq!(wallet.held_balance(), Money::from_cents(4000));
}

// ── Release ──────────────────────────────────────────────────────

#[tokio::test]
async fn release_moves_held_to_active() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.top_up("user-1", "BUYER", Money::from_cents(10000))
        .await
        .unwrap();
    svc.hold("user-1", "BUYER", Money::from_cents(5000))
        .await
        .unwrap();

    let wallet = svc
        .release("user-1", "BUYER", Money::from_cents(3000))
        .await
        .unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(8000));
    assert_eq!(wallet.held_balance(), Money::from_cents(2000));
}

// ── Convert ──────────────────────────────────────────────────────

#[tokio::test]
async fn convert_removes_held_balance() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.top_up("user-1", "BUYER", Money::from_cents(10000))
        .await
        .unwrap();
    svc.hold("user-1", "BUYER", Money::from_cents(5000))
        .await
        .unwrap();

    let wallet = svc
        .convert("user-1", "BUYER", Money::from_cents(5000))
        .await
        .unwrap();
    assert_eq!(wallet.held_balance(), Money::zero());
    assert_eq!(wallet.active_balance(), Money::from_cents(5000));
}

// ── Bid ──────────────────────────────────────────────────────────

#[tokio::test]
async fn bid_moves_active_to_held() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.top_up("user-1", "BUYER", Money::from_cents(10000))
        .await
        .unwrap();

    let wallet = svc
        .bid("user-1", "BUYER", Money::from_cents(4000))
        .await
        .unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(6000));
    assert_eq!(wallet.held_balance(), Money::from_cents(4000));
}

// ── Transaction history ──────────────────────────────────────────

#[tokio::test]
async fn transaction_history_records_all_operations() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.top_up("user-1", "BUYER", Money::from_cents(10000))
        .await
        .unwrap();
    svc.hold("user-1", "BUYER", Money::from_cents(3000))
        .await
        .unwrap();
    svc.release("user-1", "BUYER", Money::from_cents(1000))
        .await
        .unwrap();

    let history = svc
        .get_transaction_history("user-1", "BUYER")
        .await
        .unwrap();
    assert_eq!(history.len(), 3);
}

// ── Cancel bid ───────────────────────────────────────────────────

#[tokio::test]
async fn cancel_bid_releases_held_funds() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.top_up("user-1", "BUYER", Money::from_cents(10000))
        .await
        .unwrap();

    let wallet_after_bid = svc
        .bid("user-1", "BUYER", Money::from_cents(4000))
        .await
        .unwrap();
    assert_eq!(wallet_after_bid.held_balance(), Money::from_cents(4000));

    // Get the bid transaction from history to find its ID
    let history = svc
        .get_transaction_history("user-1", "BUYER")
        .await
        .unwrap();
    let bid_tx = history
        .iter()
        .find(|tx| tx.transaction_type == bidmart_wallet_service_rust::wallet::TransactionType::Bid)
        .unwrap();

    svc.cancel_bid("user-1", "BUYER", &bid_tx.id.to_string())
        .await
        .unwrap();

    let wallet = svc
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(10000));
    assert_eq!(wallet.held_balance(), Money::zero());
}

#[tokio::test]
async fn cancel_bid_wrong_user_fails() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.create_wallet("user-2", "BUYER").await.unwrap();
    svc.top_up("user-1", "BUYER", Money::from_cents(10000))
        .await
        .unwrap();
    svc.bid("user-1", "BUYER", Money::from_cents(4000))
        .await
        .unwrap();

    let history = svc
        .get_transaction_history("user-1", "BUYER")
        .await
        .unwrap();
    let bid_tx = history
        .iter()
        .find(|tx| tx.transaction_type == bidmart_wallet_service_rust::wallet::TransactionType::Bid)
        .unwrap();

    let result = svc
        .cancel_bid("user-2", "BUYER", &bid_tx.id.to_string())
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn ensure_wallet_returns_existing_wallet_without_duplicate() {
    let svc = setup_service().await;

    let first = svc.ensure_wallet("user-1", "BUYER").await.unwrap();
    let second = svc.ensure_wallet("user-1", "BUYER").await.unwrap();

    assert_eq!(first.id(), second.id());
    assert_eq!(svc.find_all().await.unwrap().len(), 1);
}

#[tokio::test]
async fn apply_midtrans_payment_result_rejects_unknown_status() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    let payment = svc
        .create_top_up_intent("user-1", "BUYER", Money::from_cents(5000), None)
        .await
        .unwrap();

    let result = svc
        .apply_midtrans_payment_result(&payment.id, "mystery")
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn simulate_payment_status_pending_is_noop() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    let payment = svc
        .create_top_up_intent("user-1", "BUYER", Money::from_cents(5000), None)
        .await
        .unwrap();

    let updated = svc
        .simulate_payment_status(&payment.id, "PENDING")
        .await
        .unwrap();
    let wallet = svc
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap();
    let history = svc
        .get_transaction_history("user-1", "BUYER")
        .await
        .unwrap();

    assert_eq!(updated.status, "PENDING");
    assert_eq!(wallet.active_balance(), Money::zero());
    assert!(history.is_empty());
}

#[tokio::test]
async fn sync_midtrans_payment_status_returns_terminal_payment_without_gateway_call() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    let payment = svc
        .create_top_up_intent("user-1", "BUYER", Money::from_cents(5000), None)
        .await
        .unwrap();
    svc.simulate_payment_status(&payment.id, "FAILED")
        .await
        .unwrap();

    let synced = svc.sync_midtrans_payment_status(&payment.id).await.unwrap();

    assert_eq!(synced.status, "FAILED");
}

#[tokio::test]
async fn simulate_withdrawal_completed_does_not_reverse_balance() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.top_up("user-1", "BUYER", Money::from_cents(10_000))
        .await
        .unwrap();
    let withdrawal = svc
        .create_withdrawal(
            "user-1",
            "BUYER",
            Money::from_cents(3000),
            "bca",
            "1234567890",
        )
        .await
        .unwrap();

    let completed = svc
        .simulate_withdrawal_status(&withdrawal.id, "COMPLETED")
        .await
        .unwrap();
    let wallet = svc
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap();

    assert_eq!(completed.status, "COMPLETED");
    assert_eq!(wallet.active_balance(), Money::from_cents(7000));
}

#[tokio::test]
async fn simulate_withdrawal_expired_reverses_balance_and_records_history() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();
    svc.top_up("user-1", "BUYER", Money::from_cents(10_000))
        .await
        .unwrap();
    let withdrawal = svc
        .create_withdrawal(
            "user-1",
            "BUYER",
            Money::from_cents(3000),
            "bca",
            "1234567890",
        )
        .await
        .unwrap();

    let expired = svc
        .simulate_withdrawal_status(&withdrawal.id, "EXPIRED")
        .await
        .unwrap();
    let wallet = svc
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap();
    let history = svc
        .get_transaction_history("user-1", "BUYER")
        .await
        .unwrap();

    assert_eq!(expired.status, "EXPIRED");
    assert_eq!(wallet.active_balance(), Money::from_cents(10_000));
    assert!(
        history
            .iter()
            .any(|tx| tx.transaction_type == TransactionType::WithdrawExpired)
    );
}

#[tokio::test]
async fn create_withdrawal_rejects_zero_amount() {
    let svc = setup_service().await;
    svc.create_wallet("user-1", "BUYER").await.unwrap();

    let result = svc
        .create_withdrawal("user-1", "BUYER", Money::zero(), "bca", "1234567890")
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn payout_to_seller_reuses_existing_seller_wallet() {
    let svc = setup_service().await;
    let created = svc.create_wallet("seller-1", "SELLER").await.unwrap();

    let paid = svc
        .payout_to_seller("seller-1", Money::from_cents(4000))
        .await
        .unwrap();

    assert_eq!(created.id(), paid.id());
    assert_eq!(paid.active_balance(), Money::from_cents(4000));
    assert_eq!(svc.find_all().await.unwrap().len(), 1);
}
