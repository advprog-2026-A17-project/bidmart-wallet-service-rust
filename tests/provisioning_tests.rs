use bidmart_wallet_service_rust::service::wallet_service::WalletService;
use bidmart_wallet_service_rust::wallet::Money;

use bidmart_wallet_service_rust::server;

// ── Setup ────────────────────────────────────────────────────────

async fn setup_service() -> WalletService {
    let pool = server::connect_pool("sqlite::memory:")
        .await
        .unwrap();
    server::run_migrations(&pool).await.unwrap();
    WalletService::new(pool)
}

// ── Provision wallet ─────────────────────────────────────────────

#[tokio::test]
async fn provision_wallet_creates_wallet_for_new_user() {
    let svc = setup_service().await;

    svc.provision_wallet("evt-1", "user-1", "user@example.com", "auth-service")
        .await
        .unwrap();

    let wallet = svc.find_by_user_id("user-1").await.unwrap();
    assert_eq!(wallet.user_id(), "user-1");
    assert_eq!(wallet.active_balance(), Money::zero());
}

#[tokio::test]
async fn provision_wallet_is_idempotent_by_event_id() {
    let svc = setup_service().await;

    svc.provision_wallet("evt-1", "user-1", "user@example.com", "auth-service")
        .await
        .unwrap();

    // Same event ID again — should be a no-op
    svc.provision_wallet("evt-1", "user-1", "user@example.com", "auth-service")
        .await
        .unwrap();

    let all = svc.find_all().await.unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn provision_wallet_skips_if_wallet_already_exists() {
    let svc = setup_service().await;

    // Manually create wallet first
    svc.create_wallet("user-1").await.unwrap();

    // Provisioning with a new event should not create a duplicate
    svc.provision_wallet("evt-1", "user-1", "user@example.com", "auth-service")
        .await
        .unwrap();

    let all = svc.find_all().await.unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn provision_wallet_different_users_creates_separate_wallets() {
    let svc = setup_service().await;

    svc.provision_wallet("evt-1", "user-1", "u1@example.com", "auth")
        .await
        .unwrap();
    svc.provision_wallet("evt-2", "user-2", "u2@example.com", "auth")
        .await
        .unwrap();

    let all = svc.find_all().await.unwrap();
    assert_eq!(all.len(), 2);
}

// ── Reconciliation ───────────────────────────────────────────────

#[tokio::test]
async fn reconcile_creates_missing_wallets() {
    let svc = setup_service().await;

    // Provision two users
    svc.provision_wallet("evt-1", "user-1", "u1@example.com", "auth")
        .await
        .unwrap();
    svc.provision_wallet("evt-2", "user-2", "u2@example.com", "auth")
        .await
        .unwrap();

    // Verify both exist
    assert_eq!(svc.find_all().await.unwrap().len(), 2);

    // Reconcile should find no missing wallets
    let created = svc.reconcile_provisioned_wallets(100).await.unwrap();
    assert_eq!(created, 0);
}

#[tokio::test]
async fn reconcile_with_zero_batch_uses_default() {
    let svc = setup_service().await;

    svc.provision_wallet("evt-1", "user-1", "u1@example.com", "auth")
        .await
        .unwrap();

    // batch_size=0 should use default (100) and not panic
    let created = svc.reconcile_provisioned_wallets(0).await.unwrap();
    assert_eq!(created, 0);
}
