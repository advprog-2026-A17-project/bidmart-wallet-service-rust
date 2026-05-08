use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::http::dto::*;
use crate::service::wallet_service::{ServiceError, WalletService};
use crate::wallet::{Money, WalletError};

type AppState = Arc<WalletService>;
const DEFAULT_INTERNAL_TOKEN: &str = "local-dev-internal-token";

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
        .route("/:userId/top-up/intent", post(create_top_up_intent))
        .route("/:userId/withdraw", post(withdraw))
        .route("/:userId/withdrawals", post(create_withdrawal))
        .route("/:userId/trybid", post(try_bid))
        .route(
            "/midtrans/payments/:paymentId/simulate",
            post(simulate_payment),
        )
        .route("/midtrans/payments/:paymentId/sync", post(sync_payment))
        .route("/midtrans/payments/return", post(record_payment_return))
        .route(
            "/midtrans/withdrawals/:withdrawalId/simulate",
            post(simulate_withdrawal),
        )
        .with_state(state);

    Router::new().nest("/api/v1/wallet", wallet_routes)
}

// ── Handlers ────────────────────────────────────────────────────

async fn get_wallet(State(svc): State<AppState>, Path(user_id): Path<String>) -> impl IntoResponse {
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
        history: history
            .iter()
            .map(WalletTransactionResponse::from)
            .collect(),
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

async fn create_top_up_intent(
    State(svc): State<AppState>,
    Path(user_id): Path<String>,
    Json(req): Json<PaymentIntentRequest>,
) -> impl IntoResponse {
    match svc
        .create_top_up_intent(
            &user_id,
            Money::from_cents(req.amount_cents),
            req.payment_method.as_deref(),
        )
        .await
    {
        Ok(payment) => Ok((
            StatusCode::CREATED,
            Json(PaymentIntentResponse::from(&payment)),
        )),
        Err(e) => Err(map_error(e)),
    }
}

async fn simulate_payment(
    State(svc): State<AppState>,
    Path(payment_id): Path<String>,
    Json(req): Json<MidtransSimulationRequest>,
) -> impl IntoResponse {
    match svc.simulate_payment_status(&payment_id, &req.status).await {
        Ok(payment) => Ok(Json(PaymentIntentResponse::from(&payment))),
        Err(e) => Err(map_error(e)),
    }
}

async fn sync_payment(
    State(svc): State<AppState>,
    Path(payment_id): Path<String>,
) -> impl IntoResponse {
    match svc.sync_midtrans_payment_status(&payment_id).await {
        Ok(payment) => Ok(Json(PaymentIntentResponse::from(&payment))),
        Err(e) => Err(map_error(e)),
    }
}

async fn record_payment_return(
    State(svc): State<AppState>,
    Json(req): Json<MidtransPaymentReturnRequest>,
) -> impl IntoResponse {
    let _ = req.status_code.as_deref();
    match svc
        .apply_midtrans_payment_result(&req.order_id, &req.transaction_status)
        .await
    {
        Ok(payment) => Ok(Json(PaymentIntentResponse::from(&payment))),
        Err(e) => Err(map_error(e)),
    }
}

async fn hold_funds(
    State(svc): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<HoldFundsRequest>,
) -> impl IntoResponse {
    require_internal_token(&headers)?;
    // Kita akan memanggil metode hold_funds di Service yang meneruskan semua data
    match svc
        .hold_funds(
            &req.user_id,
            &req.auction_id,
            &req.bid_id,
            Money::from_cents(req.amount),
            &req.hold_id,
            &req.expires_at,
        )
        .await
    {
        Ok(hold) => Ok(Json(HoldResponse::from(&hold))),
        Err(e) => Err(map_error(e)),
    }
}

async fn release_funds(
    State(svc): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ReleaseFundsRequest>,
) -> impl IntoResponse {
    require_internal_token(&headers)?;
    match svc.release_funds(&req.hold_id).await {
        Ok(hold) => Ok(Json(HoldResponse::from(&hold))),
        Err(e) => Err(map_error(e)),
    }
}

async fn convert_funds(
    State(svc): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ConvertFundsRequest>,
) -> impl IntoResponse {
    require_internal_token(&headers)?;
    match svc.convert_funds(&req.hold_id).await {
        Ok(hold) => Ok(Json(HoldResponse::from(&hold))),
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

async fn create_withdrawal(
    State(svc): State<AppState>,
    Path(user_id): Path<String>,
    Json(req): Json<WithdrawalRequest>,
) -> impl IntoResponse {
    match svc
        .create_withdrawal(
            &user_id,
            Money::from_cents(req.amount_cents),
            &req.bank_account,
        )
        .await
    {
        Ok(withdrawal) => Ok((
            StatusCode::CREATED,
            Json(WithdrawalResponse::from(&withdrawal)),
        )),
        Err(e) => Err(map_error(e)),
    }
}

async fn simulate_withdrawal(
    State(svc): State<AppState>,
    Path(withdrawal_id): Path<String>,
    Json(req): Json<MidtransSimulationRequest>,
) -> impl IntoResponse {
    match svc
        .simulate_withdrawal_status(&withdrawal_id, &req.status)
        .await
    {
        Ok(withdrawal) => Ok(Json(WithdrawalResponse::from(&withdrawal))),
        Err(e) => Err(map_error(e)),
    }
}

// ── Error mapping ───────────────────────────────────────────────

fn require_internal_token(
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<StructuredErrorResponse>)> {
    let expected_token = std::env::var("GATEWAY_INTERNAL_TOKEN")
        .unwrap_or_else(|_| DEFAULT_INTERNAL_TOKEN.to_string());
    let provided_token = headers
        .get("x-internal-service-token")
        .and_then(|value| value.to_str().ok());

    if provided_token == Some(expected_token.as_str()) {
        return Ok(());
    }

    Err((
        StatusCode::FORBIDDEN,
        Json(StructuredErrorResponse {
            error_code: "INVALID_INTERNAL_TOKEN".to_string(),
            message: "invalid internal service token".to_string(),
        }),
    ))
}

fn map_error(e: ServiceError) -> (StatusCode, Json<StructuredErrorResponse>) {
    let (status, code, message) = match &e {
        ServiceError::WalletNotFound(_) => {
            (StatusCode::NOT_FOUND, "WALLET_NOT_FOUND", e.to_string())
        }
        ServiceError::Domain(wallet_err) => {
            let code = match wallet_err {
                WalletError::InsufficientActiveBalance => "INSUFFICIENT_ACTIVE_BALANCE",
                WalletError::InsufficientHeldBalance => "INSUFFICIENT_HELD_BALANCE",
                WalletError::InvalidAmount => "INVALID_AMOUNT",
            };
            (StatusCode::BAD_REQUEST, code, e.to_string())
        }
        ServiceError::TransactionNotFound(_) => (
            StatusCode::NOT_FOUND,
            "TRANSACTION_NOT_FOUND",
            e.to_string(),
        ),
        ServiceError::ForbiddenAccess => (StatusCode::FORBIDDEN, "FORBIDDEN_ACCESS", e.to_string()),
        ServiceError::Persistence(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "internal server error".to_string(),
        ),
        ServiceError::HoldFailed(msg) => {
            let code = if msg.contains("Insufficient active balance") {
                "INSUFFICIENT_ACTIVE_BALANCE"
            } else {
                "HOLD_OPERATION_FAILED"
            };
            (StatusCode::BAD_REQUEST, code, msg.clone())
        }
        ServiceError::InvalidPaymentStatus(_) => (
            StatusCode::BAD_REQUEST,
            "INVALID_PAYMENT_STATUS",
            e.to_string(),
        ),
        ServiceError::Midtrans(_) => (StatusCode::BAD_GATEWAY, "MIDTRANS_ERROR", e.to_string()),
    };

    (
        status,
        Json(StructuredErrorResponse {
            error_code: code.to_string(),
            message,
        }),
    )
}
