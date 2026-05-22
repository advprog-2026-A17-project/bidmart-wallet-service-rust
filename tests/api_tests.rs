use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::AnyPool;
use tower::ServiceExt;

use bidmart_wallet_service_rust::server;

async fn setup_app() -> axum::Router {
    let (app, _) = setup_app_with_pool().await;
    app
}

async fn setup_app_with_pool() -> (axum::Router, AnyPool) {
    let pool = server::connect_pool("sqlite::memory:").await.unwrap();
    server::run_migrations(&pool).await.unwrap();
    (server::build_router(pool.clone()), pool)
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
async fn get_wallet_provisions_wallet_when_missing() {
    let app = setup_app().await;

    let req = Request::builder()
        .uri("/api/v1/wallet/ghost")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["userId"], "ghost");
    assert_eq!(json["activeBalance"], 0);
    assert_eq!(json["heldBalance"], 0);
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
        .body(Body::from(r#"{"amount":5000}"#))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let json = body_to_json(resp.into_body()).await;
    let payment_id = json["paymentId"].as_str().unwrap();
    assert!(!payment_id.is_empty());
    assert_eq!(json["amount"], 5000);
    assert_eq!(json["status"], "PENDING");
    assert!(json["redirectUrl"].as_str().unwrap().contains(payment_id));

    let get_req = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    let wallet = body_to_json(get_resp.into_body()).await;
    assert_eq!(wallet["activeBalance"], 0);
}

#[tokio::test]
async fn top_up_intent_idempotency_key_returns_original_payment() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let first = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up/intent")
        .header("content-type", "application/json")
        .header("Idempotency-Key", "topup-key-1")
        .body(Body::from(r#"{"amount":5000}"#))
        .unwrap();
    let first_resp = app.clone().oneshot(first).await.unwrap();
    assert_eq!(first_resp.status(), StatusCode::CREATED);
    let first_json = body_to_json(first_resp.into_body()).await;

    let second = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/top-up/intent")
        .header("content-type", "application/json")
        .header("Idempotency-Key", "topup-key-1")
        .body(Body::from(r#"{"amount":5000}"#))
        .unwrap();
    let second_resp = app.oneshot(second).await.unwrap();
    assert_eq!(second_resp.status(), StatusCode::CREATED);
    let second_json = body_to_json(second_resp.into_body()).await;

    assert_eq!(second_json["paymentId"], first_json["paymentId"]);
    assert_eq!(second_json["amount"], first_json["amount"]);
}

#[tokio::test]
async fn wallet_detail_exposes_unpaid_payment_history_and_payment_detail_expires() {
    let (app, pool) = setup_app_with_pool().await;

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
        .body(Body::from(r#"{"amount":5000,"paymentMethod":"bca_va"}"#))
        .unwrap();
    let intent_resp = app.clone().oneshot(intent).await.unwrap();
    assert_eq!(intent_resp.status(), StatusCode::CREATED);
    let intent_json = body_to_json(intent_resp.into_body()).await;
    let payment_id = intent_json["paymentId"].as_str().unwrap();
    assert_eq!(intent_json["status"], "PENDING");
    assert!(intent_json["expiresAt"].as_str().unwrap().len() > 10);

    let detail = Request::builder()
        .uri("/api/v1/wallet/user-1/detail")
        .body(Body::empty())
        .unwrap();
    let detail_resp = app.clone().oneshot(detail).await.unwrap();
    let detail_json = body_to_json(detail_resp.into_body()).await;
    let unpaid = detail_json["unpaidPayments"].as_array().unwrap();
    assert_eq!(unpaid.len(), 1);
    assert_eq!(unpaid[0]["paymentId"], payment_id);

    sqlx::query("UPDATE wallet_payment_intents SET created_at = $1 WHERE id = $2")
        .bind("2000-01-01T00:00:00Z")
        .bind(payment_id)
        .execute(&pool)
        .await
        .unwrap();

    let payment_detail = Request::builder()
        .uri(format!("/api/v1/wallet/user-1/payments/{payment_id}"))
        .body(Body::empty())
        .unwrap();
    let payment_resp = app.clone().oneshot(payment_detail).await.unwrap();
    assert_eq!(payment_resp.status(), StatusCode::OK);
    let payment_json = body_to_json(payment_resp.into_body()).await;
    assert_eq!(payment_json["status"], "EXPIRED");
    assert_eq!(payment_json["vaNumber"], serde_json::Value::Null);

    let detail_after_expiry = Request::builder()
        .uri("/api/v1/wallet/user-1/detail")
        .body(Body::empty())
        .unwrap();
    let detail_after_expiry_resp = app.clone().oneshot(detail_after_expiry).await.unwrap();
    let detail_after_expiry_json = body_to_json(detail_after_expiry_resp.into_body()).await;
    let history = detail_after_expiry_json["history"].as_array().unwrap();
    assert!(history.iter().any(|entry| {
        entry["type"] == "TOP_UP_EXPIRED" && entry["correlationId"] == payment_id
    }));
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
        .body(Body::from(r#"{"amount":7500}"#))
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
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    let wallet = body_to_json(get_resp.into_body()).await;
    assert_eq!(wallet["activeBalance"], 7500);
}

#[tokio::test]
async fn midtrans_failed_and_expired_payments_are_recorded_in_history() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    for status in ["FAILED", "EXPIRED"] {
        let intent = Request::builder()
            .method("POST")
            .uri("/api/v1/wallet/user-1/top-up/intent")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"amount":4500}"#))
            .unwrap();
        let intent_resp = app.clone().oneshot(intent).await.unwrap();
        let intent_json = body_to_json(intent_resp.into_body()).await;
        let payment_id = intent_json["paymentId"].as_str().unwrap();

        let callback = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/wallet/midtrans/payments/{payment_id}/simulate"
            ))
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"status":"{status}"}}"#)))
            .unwrap();
        let resp = app.clone().oneshot(callback).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let detail = Request::builder()
        .uri("/api/v1/wallet/user-1/detail")
        .body(Body::empty())
        .unwrap();
    let detail_resp = app.clone().oneshot(detail).await.unwrap();
    assert_eq!(detail_resp.status(), StatusCode::OK);
    let detail_json = body_to_json(detail_resp.into_body()).await;

    assert_eq!(detail_json["wallet"]["activeBalance"], 0);
    let history = detail_json["history"].as_array().unwrap();
    let types: Vec<&str> = history
        .iter()
        .map(|entry| entry["type"].as_str().unwrap())
        .collect();
    assert!(types.contains(&"TOP_UP_FAILED"));
    assert!(types.contains(&"TOP_UP_EXPIRED"));
    assert!(history.iter().all(|entry| entry["timestamp"].is_string()));
}

#[tokio::test]
async fn midtrans_return_callback_maps_web_sandbox_statuses() {
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
        .body(Body::from(r#"{"amount":9900}"#))
        .unwrap();
    let intent_resp = app.clone().oneshot(intent).await.unwrap();
    let intent_json = body_to_json(intent_resp.into_body()).await;
    let payment_id = intent_json["paymentId"].as_str().unwrap();

    let callback = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/midtrans/payments/return")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"orderId":"{payment_id}","transactionStatus":"settlement","statusCode":"200"}}"#
        )))
        .unwrap();
    let callback_resp = app.clone().oneshot(callback).await.unwrap();
    assert_eq!(callback_resp.status(), StatusCode::OK);
    let callback_json = body_to_json(callback_resp.into_body()).await;
    assert_eq!(callback_json["status"], "PAID");

    let get_req = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    let wallet = body_to_json(get_resp.into_body()).await;
    assert_eq!(wallet["activeBalance"], 9900);
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
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
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
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
        .body(Body::from(r#"{"userId":"user-1","holdId":"hold-2","auctionId":"auc-2","bidId":"bid-2","amount":5000,"expiresAt":"2026-12-31T23:59:59Z"}"#))
        .unwrap();
    let _ = app.clone().oneshot(hold).await.unwrap();

    // 1. Request Release (Hanya butuh holdId)
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/release")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
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
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
        .body(Body::from(r#"{"userId":"user-1","holdId":"hold-3","auctionId":"auc-3","bidId":"bid-3","amount":5000,"expiresAt":"2026-12-31T23:59:59Z"}"#))
        .unwrap();
    let _ = app.clone().oneshot(hold).await.unwrap();

    // 1. Request Convert
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/convert")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
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
            r#"{"amount":3000,"bankCode":"bca","accountNumber":"1234567890"}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let json = body_to_json(resp.into_body()).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let withdrawal_id = json["withdrawalId"].as_str().unwrap();
    assert_eq!(json["amount"], 3000);
    assert_eq!(json["status"], "PENDING");
    assert_eq!(json["bankCode"], "bca");
    assert_eq!(json["accountNumber"], "1234567890");
    assert_eq!(json["accountName"], "Validated Development Account");
    assert!(json["payoutReference"].as_str().unwrap().starts_with("WD-"));

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
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    let wallet = body_to_json(get_resp.into_body()).await;
    assert_eq!(wallet["activeBalance"], 10000);

    let detail = Request::builder()
        .uri("/api/v1/wallet/user-1/detail")
        .body(Body::empty())
        .unwrap();
    let detail_resp = app.oneshot(detail).await.unwrap();
    let detail_json = body_to_json(detail_resp.into_body()).await;
    let history = detail_json["history"].as_array().unwrap();
    let types: Vec<&str> = history
        .iter()
        .map(|entry| entry["type"].as_str().unwrap())
        .collect();
    assert!(types.contains(&"WITHDRAW_FAILED"));
}

#[tokio::test]
async fn withdrawal_idempotency_key_returns_original_withdrawal_without_second_debit() {
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

    let first = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/withdrawals")
        .header("content-type", "application/json")
        .header("Idempotency-Key", "withdraw-key-1")
        .body(Body::from(
            r#"{"amount":3000,"bankCode":"bca","accountNumber":"1234567890"}"#,
        ))
        .unwrap();
    let first_resp = app.clone().oneshot(first).await.unwrap();
    assert_eq!(first_resp.status(), StatusCode::CREATED);
    let first_json = body_to_json(first_resp.into_body()).await;

    let second = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/user-1/withdrawals")
        .header("content-type", "application/json")
        .header("Idempotency-Key", "withdraw-key-1")
        .body(Body::from(
            r#"{"amount":3000,"bankCode":"bca","accountNumber":"1234567890"}"#,
        ))
        .unwrap();
    let second_resp = app.clone().oneshot(second).await.unwrap();
    assert_eq!(second_resp.status(), StatusCode::CREATED);
    let second_json = body_to_json(second_resp.into_body()).await;

    assert_eq!(second_json["withdrawalId"], first_json["withdrawalId"]);

    let balance = Request::builder()
        .uri("/api/v1/wallet/user-1")
        .body(Body::empty())
        .unwrap();
    let balance_resp = app.oneshot(balance).await.unwrap();
    let balance_json = body_to_json(balance_resp.into_body()).await;
    assert_eq!(balance_json["activeBalance"], 7000);
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
async fn get_wallet_detail_auto_creates_wallet_when_missing() {
    let app = setup_app().await;

    let req = Request::builder()
        .uri("/api/v1/wallet/user-auto/detail?role=BUYER")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["wallet"]["userId"], "user-auto");
    assert_eq!(json["wallet"]["role"], "BUYER");
}

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
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
        .body(Body::from(r#"{"userId":"user-1","holdId":"hold-4","auctionId":"auc-4","bidId":"bid-4","amount":9999,"expiresAt":"2026-12-31T23:59:59Z"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    // Status Code harus 400 Bad Request
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn convert_funds_fails_when_hold_already_released() {
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

    let hold = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/hold")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
        .body(Body::from(r#"{"userId":"user-1","holdId":"hold-released","auctionId":"auc-r","bidId":"bid-r","amount":3000,"expiresAt":"2026-12-31T23:59:59Z"}"#))
        .unwrap();
    let _ = app.clone().oneshot(hold).await.unwrap();

    let release = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/release")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
        .body(Body::from(r#"{"holdId":"hold-released"}"#))
        .unwrap();
    let _ = app.clone().oneshot(release).await.unwrap();

    let convert = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/convert")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
        .body(Body::from(r#"{"holdId":"hold-released"}"#))
        .unwrap();
    let resp = app.oneshot(convert).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn seller_escrow_credit_and_payout_settlement_moves_held_to_active() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"seller-1","role":"SELLER"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let escrow = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/seller-escrow")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
        .body(Body::from(
            r#"{"sellerId":"seller-1","amount":7500,"correlationId":"auction-1"}"#,
        ))
        .unwrap();
    let escrow_resp = app.clone().oneshot(escrow).await.unwrap();
    assert_eq!(escrow_resp.status(), StatusCode::OK);
    let escrow_json = body_to_json(escrow_resp.into_body()).await;
    assert_eq!(escrow_json["heldBalance"], 7500);
    assert_eq!(escrow_json["activeBalance"], 0);

    let payout = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/payout")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "bidmart-local-internal-token")
        .body(Body::from(
            r#"{"sellerId":"seller-1","amount":7500,"orderId":"order-1"}"#,
        ))
        .unwrap();
    let payout_resp = app.clone().oneshot(payout).await.unwrap();
    assert_eq!(payout_resp.status(), StatusCode::OK);
    let payout_json = body_to_json(payout_resp.into_body()).await;
    assert_eq!(payout_json["heldBalance"], 0);
    assert_eq!(payout_json["activeBalance"], 7500);
}

#[tokio::test]
async fn concurrent_top_up_and_hold_do_not_produce_negative_balance() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"race-user"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let topup = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/race-user/top-up?amount=5000")
        .body(Body::empty())
        .unwrap();
    let _ = app.clone().oneshot(topup).await.unwrap();

    let hold_task = {
        let app = app.clone();
        tokio::spawn(async move {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/wallet/hold")
                .header("content-type", "application/json")
                .header("X-Internal-Service-Token", "bidmart-local-internal-token")
                .body(Body::from(r#"{"userId":"race-user","holdId":"race-hold","auctionId":"auc-race","bidId":"bid-race","amount":4000,"expiresAt":"2026-12-31T23:59:59Z"}"#))
                .unwrap();
            app.oneshot(req).await.unwrap().status()
        })
    };

    let withdraw_task = {
        let app = app.clone();
        tokio::spawn(async move {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/wallet/race-user/withdraw?amount=4000")
                .body(Body::empty())
                .unwrap();
            app.oneshot(req).await.unwrap().status()
        })
    };

    let (hold_status, withdraw_status) = tokio::join!(hold_task, withdraw_task);
    let hold_status = hold_status.unwrap();
    let withdraw_status = withdraw_status.unwrap();

    assert!(
        hold_status.is_success() ^ withdraw_status.is_success(),
        "expected exactly one of hold or withdraw to succeed"
    );

    let get_req = Request::builder()
        .uri("/api/v1/wallet/race-user")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get_req).await.unwrap();
    let get_json = body_to_json(get_resp.into_body()).await;
    let active = get_json["activeBalance"].as_u64().unwrap_or(0);
    let held = get_json["heldBalance"].as_u64().unwrap_or(0);
    assert!(active + held <= 5000);
}
