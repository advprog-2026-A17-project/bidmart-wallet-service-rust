use bidmart_wallet_service_rust::grpc::WalletGrpcHandler;
use bidmart_wallet_service_rust::grpc::wallet::wallet_service_server::WalletService as GrpcWalletService;
use bidmart_wallet_service_rust::grpc::wallet::{
    GrpcConvertFundsRequest, GrpcHoldFundsRequest, GrpcReleaseFundsRequest,
};
use bidmart_wallet_service_rust::server;
use bidmart_wallet_service_rust::service::wallet_service::WalletService;
use bidmart_wallet_service_rust::wallet::Money;
use tonic::Request;

async fn setup_handler() -> (WalletGrpcHandler, WalletService) {
    let pool = server::connect_pool("sqlite::memory:").await.unwrap();
    server::run_migrations(&pool).await.unwrap();
    let handler = WalletGrpcHandler::new(pool.clone());
    let service = WalletService::new(pool);
    (handler, service)
}

#[tokio::test]
async fn grpc_hold_release_and_convert_flow() {
    let (handler, service) = setup_handler().await;
    service.create_wallet("user-1", "BUYER").await.unwrap();
    service
        .top_up("user-1", "BUYER", Money::from_cents(10_000))
        .await
        .unwrap();

    let hold = GrpcWalletService::hold_funds(
        &handler,
        Request::new(GrpcHoldFundsRequest {
            user_id: "user-1".to_string(),
            role: Some("BUYER".to_string()),
            hold_id: "hold-grpc-1".to_string(),
            auction_id: "auc-grpc-1".to_string(),
            bid_id: "bid-grpc-1".to_string(),
            amount: 4000,
            expires_at: "2026-12-31T23:59:59Z".to_string(),
        }),
    )
    .await
    .unwrap()
    .into_inner();

    assert_eq!(hold.id, "hold-grpc-1");
    assert_eq!(hold.status, "HELD");
    assert_eq!(hold.amount, 4000);

    GrpcWalletService::release_hold(
        &handler,
        Request::new(GrpcReleaseFundsRequest {
            hold_id: "hold-grpc-1".to_string(),
        }),
    )
    .await
    .unwrap();

    let second_hold = GrpcWalletService::hold_funds(
        &handler,
        Request::new(GrpcHoldFundsRequest {
            user_id: "user-1".to_string(),
            role: None,
            hold_id: "hold-grpc-2".to_string(),
            auction_id: "auc-grpc-2".to_string(),
            bid_id: "bid-grpc-2".to_string(),
            amount: 3000,
            expires_at: "2026-12-31T23:59:59Z".to_string(),
        }),
    )
    .await
    .unwrap()
    .into_inner();
    assert_eq!(second_hold.id, "hold-grpc-2");

    GrpcWalletService::convert_hold_to_payment(
        &handler,
        Request::new(GrpcConvertFundsRequest {
            hold_id: "hold-grpc-2".to_string(),
        }),
    )
    .await
    .unwrap();

    let wallet = service
        .find_by_user_id_and_role("user-1", "BUYER")
        .await
        .unwrap();
    assert_eq!(wallet.active_balance(), Money::from_cents(7000));
    assert_eq!(wallet.held_balance(), Money::zero());
}

#[tokio::test]
async fn grpc_returns_internal_status_for_failed_hold_operations() {
    let (handler, _) = setup_handler().await;

    let err = GrpcWalletService::hold_funds(
        &handler,
        Request::new(GrpcHoldFundsRequest {
            user_id: "missing".to_string(),
            role: Some("BUYER".to_string()),
            hold_id: "hold-missing".to_string(),
            auction_id: "auc-missing".to_string(),
            bid_id: "bid-missing".to_string(),
            amount: 4000,
            expires_at: "2026-12-31T23:59:59Z".to_string(),
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(err.code(), tonic::Code::Internal);
    assert!(err.message().contains("wallet not found"));

    let release_err = GrpcWalletService::release_hold(
        &handler,
        Request::new(GrpcReleaseFundsRequest {
            hold_id: "missing".to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(release_err.code(), tonic::Code::Internal);

    let convert_err = GrpcWalletService::convert_hold_to_payment(
        &handler,
        Request::new(GrpcConvertFundsRequest {
            hold_id: "missing".to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(convert_err.code(), tonic::Code::Internal);
}
