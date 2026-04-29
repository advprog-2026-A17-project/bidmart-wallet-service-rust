use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::http::dto::*;
use crate::service::wallet_service::{ServiceError, WalletService};
use crate::wallet::Money;

type AppState = Arc<WalletService>;

pub fn create_router(service: WalletService) -> Router {
    let state: AppState = Arc::new(service);

    let wallet_routes = Router::new()
        .route("/add", post(add_wallet))
        .route("/hold", post(hold_funds))
        .route("/release", post(release_funds))
        .route("/convert", post(convert_funds))
        .route("/:userId", get(get_wallet))
        .route("/:userId/detail", get(get_wallet_detail))
        .route("/:userId/top-up", post(top_up))
        .route("/:userId/withdraw", post(withdraw))
        .route("/:userId/trybid", post(try_bid))
        .with_state(state);

    Router::new().nest("/api/v1/wallet", wallet_routes)
}

// ── Handlers ────────────────────────────────────────────────────

async fn get_wallet(
    State(svc): State<AppState>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    match svc.find_by_user_id(&user_id).await {
        Ok(w) => Ok(Json(WalletResponse::from(&w))),
        Err(e) => Err(map_error(e)),
    }
}

async fn get_wallet_detail(
    State(svc): State<AppState>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    let wallet = match svc.find_by_user_id(&user_id).await {
        Ok(w) => w,
        Err(e) => return Err(map_error(e)),
    };

    let history = match svc.get_transaction_history(&user_id).await {
        Ok(h) => h,
        Err(e) => return Err(map_error(e)),
    };

    Ok(Json(WalletDetailResponse {
        wallet: WalletResponse::from(&wallet),
        history: history.iter().map(WalletTransactionResponse::from).collect(),
    }))
}

async fn add_wallet(
    State(svc): State<AppState>,
    Json(req): Json<WalletCreateRequest>,
) -> impl IntoResponse {
    match svc.create_wallet(&req.user_id).await {
        Ok(w) => Ok(Json(WalletResponse::from(&w))),
        Err(e) => Err(map_error(e)),
    }
}

async fn top_up(
    State(svc): State<AppState>,
    Path(user_id): Path<String>,
    Query(q): Query<AmountQuery>,
) -> impl IntoResponse {
    match svc.top_up(&user_id, Money::from_cents(q.amount)).await {
        Ok(w) => Ok(Json(WalletResponse::from(&w))),
        Err(e) => Err(map_error(e)),
    }
}

async fn hold_funds(
    State(svc): State<AppState>,
    Json(req): Json<HoldFundsRequest>,
) -> impl IntoResponse {
    match svc.hold(&req.user_id, Money::from_cents(req.amount)).await {
        Ok(w) => Ok(Json(WalletResponse::from(&w))),
        Err(e) => Err(map_error(e)),
    }
}

async fn release_funds(
    State(svc): State<AppState>,
    Json(req): Json<ReleaseFundsRequest>,
) -> impl IntoResponse {
    match svc.release(&req.user_id, Money::from_cents(req.amount)).await {
        Ok(w) => Ok(Json(WalletResponse::from(&w))),
        Err(e) => Err(map_error(e)),
    }
}

async fn convert_funds(
    State(svc): State<AppState>,
    Json(req): Json<ConvertFundsRequest>,
) -> impl IntoResponse {
    match svc.convert(&req.user_id, Money::from_cents(req.amount)).await {
        Ok(w) => Ok(Json(WalletResponse::from(&w))),
        Err(e) => Err(map_error(e)),
    }
}

async fn try_bid(
    State(svc): State<AppState>,
    Path(user_id): Path<String>,
    Query(q): Query<AmountQuery>,
) -> impl IntoResponse {
    match svc.bid(&user_id, Money::from_cents(q.amount)).await {
        Ok(w) => Ok(Json(WalletResponse::from(&w))),
        Err(e) => Err(map_error(e)),
    }
}

async fn withdraw(
    State(svc): State<AppState>,
    Path(user_id): Path<String>,
    Query(q): Query<AmountQuery>,
) -> impl IntoResponse {
    match svc.withdraw(&user_id, Money::from_cents(q.amount)).await {
        Ok(w) => Ok(Json(WalletResponse::from(&w))),
        Err(e) => Err(map_error(e)),
    }
}

// ── Error mapping ───────────────────────────────────────────────

fn map_error(e: ServiceError) -> (StatusCode, Json<ErrorResponse>) {
    let (status, message) = match &e {
        ServiceError::WalletNotFound(_) => (StatusCode::NOT_FOUND, e.to_string()),
        ServiceError::Domain(_) => (StatusCode::BAD_REQUEST, e.to_string()),
        ServiceError::TransactionNotFound(_) => (StatusCode::NOT_FOUND, e.to_string()),
        ServiceError::ForbiddenAccess => (StatusCode::FORBIDDEN, e.to_string()),
        ServiceError::Persistence(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "internal server error".to_string())
        }
    };
    (status, Json(ErrorResponse { error: message }))
}
