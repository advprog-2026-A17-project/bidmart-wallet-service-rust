use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use bidmart_wallet_service_rust::server;

async fn setup_app() -> axum::Router {
    let pool = server::connect_pool("sqlite::memory:")
        .await
        .unwrap();
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

// ── POST /api/v1/wallet/hold ─────────────────────────────────────

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

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/hold")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1","amount":4000}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["activeBalance"], 6000);
    assert_eq!(json["heldBalance"], 4000);
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

    let hold = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/hold")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1","amount":5000}"#))
        .unwrap();
    let _ = app.clone().oneshot(hold).await.unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/release")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1","amount":3000}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["activeBalance"], 8000);
    assert_eq!(json["heldBalance"], 2000);
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

    let hold = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/hold")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1","amount":5000}"#))
        .unwrap();
    let _ = app.clone().oneshot(hold).await.unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/convert")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1","amount":5000}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let json = body_to_json(resp.into_body()).await;
    assert_eq!(json["activeBalance"], 5000);
    assert_eq!(json["heldBalance"], 0);
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

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/wallet/hold")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"userId":"user-1","amount":9999}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}