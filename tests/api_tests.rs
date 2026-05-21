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
async fn get_wallet_auto_creates_missing_wallet() {
    let app = setup_app().await;

    let req = Request::builder()
        .uri("/api/v1/wallet/ghost")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["userId"], "ghost");
    assert_eq!(json["role"], "BUYER");
    assert_eq!(json["activeBalance"], 0);
    assert_eq!(json["heldBalance"], 0);
}

#[tokio::test]
async fn metrics_endpoint_exposes_wallet_service_metrics() {
    let app = setup_app().await;

    let req = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("bidmart_service_up{service=\"wallet\"} 1"));
    assert!(body.contains("bidmart_service_uptime_seconds{service=\"wallet\"}"));
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
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    let wallet = body_to_json(get_resp.into_body()).await;
    assert_eq!(wallet["activeBalance"], 0);
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
        .body(Body::from(
            r#"{"amountCents":5000,"paymentMethod":"bca_va"}"#,
        ))
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
            .body(Body::from(r#"{"amountCents":4500}"#))
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
        .body(Body::from(r#"{"amountCents":9900}"#))
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

#[tokio::test]
async fn payment_detail_rejects_other_user() {
    let app = setup_app().await;

    let create = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/add")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"owner"}"#))
        .unwrap();
    let _ = app.clone().oneshot(create).await.unwrap();

    let intent = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/owner/top-up/intent")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"amountCents":5000}"#))
        .unwrap();
    let intent_resp = app.clone().oneshot(intent).await.unwrap();
    let intent_json = body_to_json(intent_resp.into_body()).await;
    let payment_id = intent_json["paymentId"].as_str().unwrap();

    let req = Request::builder()
        .uri(format!("/api/v1/wallet/intruder/payments/{payment_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["errorCode"], "FORBIDDEN_ACCESS");
}

#[tokio::test]
async fn invalid_payment_and_withdrawal_statuses_return_400() {
    let app = setup_app().await;

    let payment = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/midtrans/payments/missing/simulate")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"status":"BOUNCED"}"#))
        .unwrap();
    let payment_resp = app.clone().oneshot(payment).await.unwrap();
    assert_eq!(payment_resp.status(), StatusCode::BAD_REQUEST);
    let payment_json = body_to_json(payment_resp.into_body()).await;
    assert_eq!(payment_json["errorCode"], "INVALID_PAYMENT_STATUS");

    let withdrawal = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/midtrans/withdrawals/missing/simulate")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"status":"BOUNCED"}"#))
        .unwrap();
    let withdrawal_resp = app.oneshot(withdrawal).await.unwrap();
    assert_eq!(withdrawal_resp.status(), StatusCode::BAD_REQUEST);
    let withdrawal_json = body_to_json(withdrawal_resp.into_body()).await;
    assert_eq!(withdrawal_json["errorCode"], "INVALID_PAYMENT_STATUS");
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

#[tokio::test]
async fn hold_funds_is_idempotent_for_same_auction_bid() {
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

    for hold_id in ["hold-original", "hold-duplicate"] {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/wallet/hold")
            .header("content-type", "application/json")
            .header("X-Internal-Service-Token", "local-dev-internal-token")
            .body(Body::from(format!(
                r#"{{"userId":"user-1","holdId":"{hold_id}","auctionId":"auc-idem","bidId":"bid-idem","amount":4000,"expiresAt":"2026-12-31T23:59:59Z"}}"#
            )))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_to_json(resp.into_body()).await;
        assert_eq!(json["id"], "hold-original");
    }

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

#[tokio::test]
async fn release_and_convert_missing_hold_return_400() {
    let app = setup_app().await;

    for (uri, body) in [
        ("/api/v1/wallet/release", r#"{"holdId":"missing"}"#),
        ("/api/v1/wallet/convert", r#"{"holdId":"missing"}"#),
    ] {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .header("X-Internal-Service-Token", "local-dev-internal-token")
            .body(Body::from(body))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_to_json(resp.into_body()).await;
        assert_eq!(json["errorCode"], "HOLD_OPERATION_FAILED");
    }
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
            r#"{"amountCents":3000,"bankCode":"bca","accountNumber":"1234567890"}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let json = body_to_json(resp.into_body()).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let withdrawal_id = json["withdrawalId"].as_str().unwrap();
    assert_eq!(json["amountCents"], 3000);
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
async fn withdrawal_validation_errors_return_400() {
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
        .uri("/api/v1/wallet/user-1/withdrawals")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"amountCents":3000,"bankCode":"unknown","accountNumber":"1234567890"}"#,
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["errorCode"], "INVALID_PAYMENT_STATUS");
}

#[tokio::test]
async fn payout_seller_requires_internal_token_and_creates_seller_wallet() {
    let app = setup_app().await;

    let forbidden = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/payout")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"sellerId":"seller-1","amountCents":8000}"#))
        .unwrap();
    let forbidden_resp = app.clone().oneshot(forbidden).await.unwrap();
    assert_eq!(forbidden_resp.status(), StatusCode::FORBIDDEN);

    let payout = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/payout")
        .header("content-type", "application/json")
        .header("X-Internal-Service-Token", "local-dev-internal-token")
        .body(Body::from(r#"{"sellerId":"seller-1","amountCents":8000}"#))
        .unwrap();
    let payout_resp = app.clone().oneshot(payout).await.unwrap();
    assert_eq!(payout_resp.status(), StatusCode::OK);
    let payout_json = body_to_json(payout_resp.into_body()).await;
    assert_eq!(payout_json["userId"], "seller-1");
    assert_eq!(payout_json["role"], "SELLER");
    assert_eq!(payout_json["activeBalance"], 8000);

    let get_seller = Request::builder()
        .uri("/api/v1/wallet/seller-1?role=SELLER")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get_seller).await.unwrap();
    let get_json = body_to_json(get_resp.into_body()).await;
    assert_eq!(get_json["activeBalance"], 8000);
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
        .header("X-Internal-Service-Token", "local-dev-internal-token")
        .body(Body::from(r#"{"userId":"user-1","holdId":"hold-4","auctionId":"auc-4","bidId":"bid-4","amount":9999,"expiresAt":"2026-12-31T23:59:59Z"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    // Status Code harus 400 Bad Request
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
