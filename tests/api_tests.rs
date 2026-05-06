use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use bidmart_wallet_service_rust::server;

async fn setup_app() -> axum::Router {
    let pool = server::connect_pool("sqlite::memory:").await.unwrap();
    server::run_migrations(&pool).await.unwrap();
    server::build_router(pool)
}

async fn body_to_json(body: Body) -> serde_json::Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ── POST /api/v1/wallet/add ──────────────────────────────────────

#[tokio::test]
async fn add_wallet_returns_ok() {
    let app = setup_app().await;

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["userId"], "user-1");
    assert_eq!(json["activeBalance"], 0);
    assert_eq!(json["heldBalance"], 0);
}

// ── GET /api/v1/wallet/{userId} ──────────────────────────────────

#[tokio::test]
async fn get_wallet_returns_ok() {
    let app = setup_app().await;

    // First create the wallet
    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    // Then fetch it
    let req = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["userId"], "user-1");
}

#[tokio::test]
async fn get_wallet_not_found_returns_404() {
    let app = setup_app().await;

    let req = Request::builder()
        .uri("/api/v1/wallet/ghost")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── POST /api/v1/wallet/{userId}/top-up ──────────────────────────

#[tokio::test]
async fn top_up_returns_updated_balance() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up?amount=5000")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["activeBalance"], 5000);
}

// ── Midtrans sandbox top-up simulation ───────────────────────────

#[tokio::test]
async fn midtrans_top_up_intent_returns_pending_payment_without_crediting_wallet() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up/intent")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"amountCents":5000}"#))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let json = body_to_json(resp.into_body()).await;
    let payment_id = json["paymentId"].as_str().unwrap();
    assert!(!payment_id.is_empty());
    assert_eq!(json["amountCents"], 5000);
    assert_eq!(json["status"], "PENDING");
    assert!(json["redirectUrl"].as_str().unwrap().contains(payment_id));

    let get_req = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get_req).await.unwrap();
    let wallet = body_to_json(get_resp.into_body()).await;
    assert_eq!(wallet["activeBalance"], 0);
}

#[tokio::test]
async fn midtrans_paid_callback_credits_wallet_once() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let intent = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up/intent")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"amountCents":7500}"#))
        .unwrap();
    let intent_resp = app.clone().oneshot(intent).await.unwrap();
    let intent_json = body_to_json(intent_resp.into_body()).await;
    let payment_id = intent_json["paymentId"].as_str().unwrap();

    for _ in 0..2 {
        let callback = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/wallet/midtrans/payments/{payment_id}/simulate"
            ))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"status":"PAID"}"#))
            .unwrap();
        let resp = app.clone().oneshot(callback).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let get_req = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get_req).await.unwrap();
    let wallet = body_to_json(get_resp.into_body()).await;
    assert_eq!(wallet["activeBalance"], 7500);
}

// ── POST /api/v1/wallet/hold ─────────────────────────────────────

#[tokio::test]
async fn hold_funds_without_internal_token_is_forbidden() {
    let app = setup_app().await;

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/hold")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1","holdId":"hold-denied","auctionId":"auc-denied","bidId":"bid-denied","amount":4000,"expiresAt":"2026-12-31T23:59:59Z"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["errorCode"], "INVALID_INTERNAL_TOKEN");
}

#[tokio::test]
async fn hold_funds_returns_updated_balances() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let topup = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up?amount=10000")
        .body(Body::empty())
        .unwrap();
    let _ = app.clone().oneshot(topup).await.unwrap();

    // 1. Payload Hold Baru
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/hold")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "local-dev-internal-token")
        .body(Body::from(r#"{"userId":"user-1","holdId":"hold-1","auctionId":"auc-1","bidId":"bid-1","amount":4000,"expiresAt":"2026-12-31T23:59:59Z"}"#))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Cek HoldResponse
    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["status"], "ACTIVE");
    assert_eq!(json["amount"], 4000);

    // 3. Verifikasi Saldo Wallet
    let get_req = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get_req).await.unwrap();
    let get_json = body_to_json(get_resp.into_body()).await;
    assert_eq!(get_json["activeBalance"], 6000);
    assert_eq!(get_json["heldBalance"], 4000);
}

// ── POST /api/v1/wallet/release ──────────────────────────────────

#[tokio::test]
async fn release_funds_returns_updated_balances() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let topup = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up?amount=10000")
        .body(Body::empty())
        .unwrap();
    let _ = app.clone().oneshot(topup).await.unwrap();

    // Setup: Buat hold dulu dengan payload lengkap
    let hold = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/hold")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "local-dev-internal-token")
        .body(Body::from(r#"{"userId":"user-1","holdId":"hold-2","auctionId":"auc-2","bidId":"bid-2","amount":5000,"expiresAt":"2026-12-31T23:59:59Z"}"#))
        .unwrap();
    let _ = app.clone().oneshot(hold).await.unwrap();

    // 1. Request Release (Hanya butuh holdId)
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/release")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "local-dev-internal-token")
        .body(Body::from(r#"{"holdId":"hold-2"}"#))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Cek HoldResponse Status
    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["status"], "RELEASED");

    // 3. Verifikasi Saldo Kembali Penuh
    let get_req = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get_req).await.unwrap();
    let get_json = body_to_json(get_resp.into_body()).await;
    assert_eq!(get_json["activeBalance"], 10000);
    assert_eq!(get_json["heldBalance"], 0);
}

// ── POST /api/v1/wallet/convert ──────────────────────────────────

#[tokio::test]
async fn convert_funds_returns_updated_balances() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let topup = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up?amount=10000")
        .body(Body::empty())
        .unwrap();
    let _ = app.clone().oneshot(topup).await.unwrap();

    // Setup Hold
    let hold = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/hold")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "local-dev-internal-token")
        .body(Body::from(r#"{"userId":"user-1","holdId":"hold-3","auctionId":"auc-3","bidId":"bid-3","amount":5000,"expiresAt":"2026-12-31T23:59:59Z"}"#))
        .unwrap();
    let _ = app.clone().oneshot(hold).await.unwrap();

    // 1. Request Convert
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/convert")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "local-dev-internal-token")
        .body(Body::from(r#"{"holdId":"hold-3"}"#))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Cek HoldResponse Status
    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["status"], "CONVERTED");

    // 3. Verifikasi Saldo Tertahan Hilang
    let get_req = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get_req).await.unwrap();
    let get_json = body_to_json(get_resp.into_body()).await;
    assert_eq!(get_json["activeBalance"], 5000);
    assert_eq!(get_json["heldBalance"], 0);
}

// ── POST /api/v1/wallet/{userId}/withdraw ────────────────────────

#[tokio::test]
async fn withdraw_returns_updated_balance() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let topup = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up?amount=10000")
        .body(Body::empty())
        .unwrap();
    let _ = app.clone().oneshot(topup).await.unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/withdraw?amount=3000")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["activeBalance"], 7000);
}

// ── Midtrans sandbox withdrawal simulation ───────────────────────

#[tokio::test]
async fn midtrans_failed_withdrawal_reverses_reserved_balance() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let topup = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up?amount=10000")
        .body(Body::empty())
        .unwrap();
    let _ = app.clone().oneshot(topup).await.unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/withdrawals")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"amountCents":3000,"bankAccount":"1234567890"}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let json = body_to_json(resp.into_body()).await;
    let withdrawal_id = json["withdrawalId"].as_str().unwrap();
    assert_eq!(json["amountCents"], 3000);
    assert_eq!(json["status"], "PENDING");

    let after_reserve = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let after_reserve_resp = app.clone().oneshot(after_reserve).await.unwrap();
    let after_reserve_json = body_to_json(after_reserve_resp.into_body()).await;
    assert_eq!(after_reserve_json["activeBalance"], 7000);

    let simulate = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1/wallet/midtrans/withdrawals/{withdrawal_id}/simulate"
        ))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"status":"FAILED"}"#))
        .unwrap();
    let simulate_resp = app.clone().oneshot(simulate).await.unwrap();
    assert_eq!(simulate_resp.status(), StatusCode::OK);

    let get_req = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get_req).await.unwrap();
    let wallet = body_to_json(get_resp.into_body()).await;
    assert_eq!(wallet["activeBalance"], 10000);
}

// ── POST /api/v1/wallet/{userId}/trybid ──────────────────────────

#[tokio::test]
async fn trybid_returns_updated_balances() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let topup = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up?amount=10000")
        .body(Body::empty())
        .unwrap();
    let _ = app.clone().oneshot(topup).await.unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/trybid?amount=4000")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["activeBalance"], 6000);
    assert_eq!(json["heldBalance"], 4000);
}

// ── GET /api/v1/wallet/{userId}/detail ───────────────────────────

#[tokio::test]
async fn get_wallet_detail_includes_history() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let topup = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up?amount=5000")
        .body(Body::empty())
        .unwrap();
    let _ = app.clone().oneshot(topup).await.unwrap();

    let req = Request::builder()
        .uri("/api/v1/wallet/user-1/detail")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["wallet"]["userId"], "user-1");
    assert_eq!(json["wallet"]["activeBalance"], 5000);
    let history = json["history"].as_array().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["type"], "TOP_UP");
}

// ── Insufficient balance returns 400 ─────────────────────────────

#[tokio::test]
async fn hold_insufficient_balance_returns_400() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    // Request ditolak karena saldonya masih 0, tapi payloadnya harus lengkap
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/hold")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "local-dev-internal-token")
        .body(Body::from(r#"{"userId":"user-1","holdId":"hold-4","auctionId":"auc-4","bidId":"bid-4","amount":9999,"expiresAt":"2026-12-31T23:59:59Z"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    // Status Code harus 400 Bad Request
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
