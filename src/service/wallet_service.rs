use sqlx::AnyPool;

use crate::payment::{
    GatewayError, MidtransGateway, map_midtrans_transaction_status, normalize_account_number,
    normalize_bank_code,
};
use crate::persistence::repositories::{
    ProvisioningEventRepository, TransactionRepository, WalletRepository,
};
use crate::wallet::{
    Hold, Money, PaymentIntent, TransactionType, Wallet, WalletError, WalletTransaction,
    WalletWithdrawal,
};

const AUCTION_SERVICE_SOURCE: &str = "auction-service";
const ORDER_SERVICE_SOURCE: &str = "order-service";

// ── ServiceError ────────────────────────────────────────────────

/// Service-level error combining domain and persistence failures.
#[derive(Debug)]
pub enum ServiceError {
    WalletNotFound(String, String),
    Domain(WalletError),
    Persistence(sqlx::Error),
    TransactionNotFound(String),
    ForbiddenAccess,
    HoldFailed(String),
    InvalidPaymentStatus(String),
    Midtrans(String),
}

impl From<WalletError> for ServiceError {
    fn from(e: WalletError) -> Self {
        Self::Domain(e)
    }
}

impl From<sqlx::Error> for ServiceError {
    fn from(e: sqlx::Error) -> Self {
        Self::Persistence(e)
    }
}

impl From<GatewayError> for ServiceError {
    fn from(e: GatewayError) -> Self {
        Self::Midtrans(e.0)
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WalletNotFound(uid, role) => {
                write!(f, "wallet not found for user {uid} with role {role}")
            }
            Self::Domain(e) => write!(f, "{e}"),
            Self::Persistence(e) => write!(f, "persistence error: {e}"),
            Self::TransactionNotFound(id) => write!(f, "transaction not found: {id}"),
            Self::ForbiddenAccess => write!(f, "forbidden transaction access"),
            Self::HoldFailed(msg) => write!(f, "hold operation failed: {msg}"),
            Self::InvalidPaymentStatus(status) => write!(f, "invalid payment status: {status}"),
            Self::Midtrans(message) => write!(f, "midtrans error: {message}"),
        }
    }
}

// ── WalletService ───────────────────────────────────────────────

/// Orchestrates wallet use cases by coordinating domain logic and persistence.
///
/// Follows the Service Layer pattern — controllers call into this layer,
/// which fetches domain entities, invokes domain operations, and persists
/// the resulting state and transaction records.
///
/// Payment integration is fully delegated to `MidtransGateway` (Facade Pattern).
pub struct WalletService {
    wallet_repo: WalletRepository,
    tx_repo: TransactionRepository,
    prov_repo: ProvisioningEventRepository,
    /// Facade to the Midtrans API.
    midtrans: MidtransGateway,
}

impl WalletService {
    pub fn new(pool: AnyPool) -> Self {
        Self {
            wallet_repo: WalletRepository::new(pool.clone()),
            tx_repo: TransactionRepository::new(pool.clone()),
            prov_repo: ProvisioningEventRepository::new(pool),
            midtrans: MidtransGateway::from_env(),
        }
    }

    // ── Queries ──────────────────────────────────────────────────

    pub async fn find_by_user_id_and_role(
        &self,
        user_id: &str,
        role: &str,
    ) -> Result<Wallet, ServiceError> {
        self.wallet_repo
            .find_by_user_id_and_role(user_id, role)
            .await?
            .ok_or_else(|| ServiceError::WalletNotFound(user_id.to_string(), role.to_string()))
    }

    pub async fn find_all(&self) -> Result<Vec<Wallet>, ServiceError> {
        Ok(self.wallet_repo.find_all().await?)
    }

    pub async fn get_transaction_history(
        &self,
        user_id: &str,
        role: &str,
    ) -> Result<Vec<WalletTransaction>, ServiceError> {
        let history = self.tx_repo.find_by_user_id_and_role(user_id, role).await?;
        Ok(dedupe_top_up_history(history))
    }

    pub async fn get_unpaid_payment_intents(
        &self,
        user_id: &str,
    ) -> Result<Vec<PaymentIntent>, ServiceError> {
        let payments = self
            .wallet_repo
            .find_unpaid_payment_intents_by_user(user_id)
            .await?;
        let mut reconciled = Vec::with_capacity(payments.len());

        for payment in payments {
            reconciled.push(self.expire_payment_if_needed(payment).await?);
        }

        Ok(reconciled)
    }

    pub async fn get_payment_intent_for_user(
        &self,
        user_id: &str,
        payment_id: &str,
    ) -> Result<PaymentIntent, ServiceError> {
        let payment = self
            .wallet_repo
            .find_payment_intent(payment_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(payment_id.to_string()))?;

        if payment.user_id != user_id {
            return Err(ServiceError::ForbiddenAccess);
        }

        self.expire_payment_if_needed(payment).await
    }

    // ── Commands ─────────────────────────────────────────────────

    pub async fn create_wallet(&self, user_id: &str, role: &str) -> Result<Wallet, ServiceError> {
        if let Ok(Some(existing)) = self
            .wallet_repo
            .find_by_user_id_and_role(user_id, role)
            .await
        {
            return Ok(existing);
        }
        let wallet = Wallet::new(user_id, role);
        self.wallet_repo.insert(&wallet).await?;
        Ok(wallet)
    }

    /// Returns an existing wallet or creates one for the user's marketplace role.
    pub async fn ensure_wallet(&self, user_id: &str, role: &str) -> Result<Wallet, ServiceError> {
        self.create_wallet(user_id, role).await
    }

    pub async fn top_up(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, role, |w| w.top_up(amount))
            .await
    }

    pub async fn withdraw(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, role, |w| w.withdraw(amount))
            .await
    }

    /// Creates a top-up payment intent via Midtrans (delegated to the Facade).
    pub async fn create_top_up_intent(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
        payment_method: Option<&str>,
        idempotency_key: Option<&str>,
    ) -> Result<PaymentIntent, ServiceError> {
        if amount.is_zero() {
            return Err(ServiceError::Domain(WalletError::InvalidAmount));
        }
        self.find_by_user_id_and_role(user_id, role).await?;
        let idempotency_key = normalize_idempotency_key(idempotency_key);
        if let Some(key) = idempotency_key.as_deref() {
            if let Some(existing) = self
                .wallet_repo
                .find_payment_intent_by_idempotency_key(user_id, role, key)
                .await?
            {
                return Ok(existing);
            }
        }

        let payment_id = idempotency_key
            .as_deref()
            .map(|key| idempotent_operation_id("pay", user_id, role, key))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Facade call — no HTTP details here
        let page = self
            .midtrans
            .create_payment(&payment_id, amount, payment_method)
            .await?;

        match self
            .wallet_repo
            .insert_payment_intent(
                &payment_id,
                user_id,
                role,
                amount,
                &page.redirect_url,
                page.va_number.as_deref(),
                page.payment_channel.as_deref(),
                idempotency_key.as_deref(),
            )
            .await
        {
            Ok(payment) => Ok(payment),
            Err(error) => {
                if let Some(key) = idempotency_key.as_deref() {
                    if let Some(existing) = self
                        .wallet_repo
                        .find_payment_intent_by_idempotency_key(user_id, role, key)
                        .await?
                    {
                        return Ok(existing);
                    }
                }
                Err(ServiceError::Persistence(error))
            }
        }
    }

    pub async fn simulate_payment_status(
        &self,
        payment_id: &str,
        status: &str,
    ) -> Result<PaymentIntent, ServiceError> {
        let normalized = status.to_ascii_uppercase();
        self.apply_payment_status(payment_id, &normalized).await
    }

    pub async fn apply_midtrans_payment_result(
        &self,
        order_id: &str,
        transaction_status: &str,
    ) -> Result<PaymentIntent, ServiceError> {
        let normalized = map_midtrans_transaction_status(transaction_status)
            .map_err(ServiceError::InvalidPaymentStatus)?;
        self.apply_payment_status(order_id, &normalized).await
    }

    pub async fn sync_midtrans_payment_status(
        &self,
        payment_id: &str,
    ) -> Result<PaymentIntent, ServiceError> {
        let payment = self
            .wallet_repo
            .find_payment_intent(payment_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(payment_id.to_string()))?;

        let payment = self.expire_payment_if_needed(payment).await?;
        if payment.status != "PENDING" {
            return Ok(payment);
        }

        // Facade call — no URL or auth details here
        let transaction_status = self.midtrans.fetch_transaction_status(payment_id).await?;

        let normalized = map_midtrans_transaction_status(&transaction_status)
            .map_err(ServiceError::InvalidPaymentStatus)?;
        self.apply_payment_status(payment_id, &normalized).await
    }

    async fn apply_payment_status(
        &self,
        payment_id: &str,
        normalized: &str,
    ) -> Result<PaymentIntent, ServiceError> {
        if !matches!(normalized, "PAID" | "FAILED" | "EXPIRED" | "PENDING") {
            return Err(ServiceError::InvalidPaymentStatus(normalized.to_string()));
        }

        self.wallet_repo
            .apply_payment_status(payment_id, normalized)
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => {
                    ServiceError::TransactionNotFound(payment_id.to_string())
                }
                other => ServiceError::Persistence(other),
            })
    }

    async fn expire_payment_if_needed(
        &self,
        payment: PaymentIntent,
    ) -> Result<PaymentIntent, ServiceError> {
        if payment.status == "PENDING" && payment_is_expired(&payment.created_at) {
            return self.apply_payment_status(&payment.id, "EXPIRED").await;
        }

        Ok(payment)
    }

    /// Creates a withdrawal and dispatches an IRIS payout (delegated to the Facade).
    pub async fn create_withdrawal(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
        bank_code: &str,
        account_number: &str,
        idempotency_key: Option<&str>,
    ) -> Result<WalletWithdrawal, ServiceError> {
        if amount.is_zero() {
            return Err(ServiceError::Domain(WalletError::InvalidAmount));
        }
        let idempotency_key = normalize_idempotency_key(idempotency_key);
        if let Some(key) = idempotency_key.as_deref() {
            if let Some(existing) = self
                .wallet_repo
                .find_withdrawal_by_idempotency_key(user_id, role, key)
                .await?
            {
                return Ok(existing);
            }
        }

        let bank_code =
            normalize_bank_code(bank_code).map_err(ServiceError::InvalidPaymentStatus)?;
        let account_number =
            normalize_account_number(account_number).map_err(ServiceError::InvalidPaymentStatus)?;

        // Facade calls — IRIS API details are hidden inside MidtransGateway
        let validated_account = self
            .midtrans
            .validate_bank_account(&bank_code, &account_number)
            .await?;

        let payout = self
            .midtrans
            .create_payout(user_id, amount, &validated_account)
            .await?;

        if payout.reference_no.trim().is_empty() {
            return Err(ServiceError::InvalidPaymentStatus(
                "missing payout reference".to_string(),
            ));
        }

        self.withdraw(user_id, role, amount).await?;
        let withdrawal_id = idempotency_key
            .as_deref()
            .map(|key| idempotent_operation_id("wd", user_id, role, key))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        match self
            .wallet_repo
            .insert_withdrawal(
                &withdrawal_id,
                user_id,
                role,
                amount,
                &validated_account.bank_code,
                &validated_account.account_number,
                &validated_account.account_name,
                &payout.reference_no,
                idempotency_key.as_deref(),
            )
            .await
        {
            Ok(withdrawal) => Ok(withdrawal),
            Err(error) => {
                if let Some(key) = idempotency_key.as_deref() {
                    if let Some(existing) = self
                        .wallet_repo
                        .find_withdrawal_by_idempotency_key(user_id, role, key)
                        .await?
                    {
                        let _ = self.top_up(user_id, role, amount).await;
                        return Ok(existing);
                    }
                }
                Err(ServiceError::Persistence(error))
            }
        }
    }

    pub async fn simulate_withdrawal_status(
        &self,
        withdrawal_id: &str,
        status: &str,
    ) -> Result<WalletWithdrawal, ServiceError> {
        let normalized = status.to_ascii_uppercase();
        if !matches!(normalized.as_str(), "COMPLETED" | "FAILED" | "EXPIRED") {
            return Err(ServiceError::InvalidPaymentStatus(status.to_string()));
        }

        let withdrawal = self
            .wallet_repo
            .find_withdrawal(withdrawal_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(withdrawal_id.to_string()))?;

        if withdrawal.status == "PENDING" {
            if normalized == "FAILED" || normalized == "EXPIRED" {
                let amount = Money::from_rupiah(withdrawal.amount as u64);
                self.top_up(&withdrawal.user_id, &withdrawal.role, amount)
                    .await?;
                let tx_type = if normalized == "FAILED" {
                    TransactionType::WithdrawFailed
                } else {
                    TransactionType::WithdrawExpired
                };
                self.record_status_transaction(
                    &withdrawal.user_id,
                    &withdrawal.role,
                    tx_type,
                    amount,
                    withdrawal_id,
                )
                .await?;
            }
            self.wallet_repo
                .update_withdrawal_status(withdrawal_id, &normalized)
                .await?;
        }

        self.wallet_repo
            .find_withdrawal(withdrawal_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(withdrawal_id.to_string()))
    }

    pub async fn hold(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, role, |w| w.hold(amount)).await
    }

    pub async fn release(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, role, |w| w.release(amount))
            .await
    }

    pub async fn convert(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, role, |w| w.convert(amount))
            .await
    }

    pub async fn bid(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, role, |w| w.bid(amount)).await
    }

    pub async fn cancel_bid(
        &self,
        user_id: &str,
        role: &str,
        bid_tx_id: &str,
    ) -> Result<(), ServiceError> {
        let tx = self
            .tx_repo
            .find_by_id(bid_tx_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(bid_tx_id.to_string()))?;

        if tx.user_id.as_ref() != user_id || tx.role.as_ref() != role {
            return Err(ServiceError::ForbiddenAccess);
        }

        self.mutate_wallet(user_id, role, |w| w.release(tx.amount))
            .await?;
        Ok(())
    }

    pub async fn hold_funds(
        &self,
        user_id: &str,
        role: &str,
        auction_id: &str,
        bid_id: &str,
        amount: Money,
        hold_id: &str,
        expires_at: &str,
    ) -> Result<Hold, ServiceError> {
        let wallet = self.find_by_user_id_and_role(user_id, role).await?;

        self.wallet_repo
            .hold_funds(wallet.id(), auction_id, bid_id, amount, hold_id, expires_at)
            .await
            .map_err(|e| ServiceError::HoldFailed(e))
    }

    pub async fn release_funds(&self, hold_id: &str) -> Result<Hold, ServiceError> {
        self.wallet_repo
            .release_funds(hold_id)
            .await
            .map_err(|e| ServiceError::HoldFailed(e))
    }

    pub async fn convert_funds(&self, hold_id: &str) -> Result<Hold, ServiceError> {
        self.wallet_repo
            .convert_funds(hold_id)
            .await
            .map_err(|e| ServiceError::HoldFailed(e))
    }

    pub async fn credit_seller_escrow(
        &self,
        seller_id: &str,
        amount: Money,
        correlation_id: Option<&str>,
    ) -> Result<Wallet, ServiceError> {
        let wallet = if let Some(wallet) = self
            .wallet_repo
            .find_by_user_id_and_role(seller_id, "SELLER")
            .await?
        {
            wallet
        } else {
            let wallet = Wallet::new(seller_id, "SELLER");
            self.wallet_repo.insert(&wallet).await?;
            wallet
        };

        let correlation_id = normalized_correlation_id(correlation_id);
        if let Some(correlation_id) = correlation_id {
            if self
                .tx_repo
                .find_by_source_correlation_and_type(
                    AUCTION_SERVICE_SOURCE,
                    correlation_id,
                    TransactionType::SellerEscrow,
                )
                .await?
                .is_some()
            {
                return Ok(wallet);
            }
        }

        self.wallet_repo
            .credit_seller_escrow_idempotent(
                seller_id,
                "SELLER",
                amount,
                correlation_id,
                AUCTION_SERVICE_SOURCE,
            )
            .await
            .map_err(ServiceError::HoldFailed)
    }

    /// Settles pending sale proceeds from held to active (used after order confirmation).
    pub async fn settle_seller_escrow(
        &self,
        seller_id: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        if self
            .wallet_repo
            .find_by_user_id_and_role(seller_id, "SELLER")
            .await?
            .is_none()
        {
            return Err(ServiceError::WalletNotFound(
                seller_id.to_string(),
                "SELLER".to_string(),
            ));
        }

        self.mutate_wallet(seller_id, "SELLER", |w| w.settle_seller_escrow(amount))
            .await
    }

    pub async fn release_seller_payout(
        &self,
        seller_id: &str,
        amount: Money,
        order_id: Option<&str>,
    ) -> Result<Wallet, ServiceError> {
        let wallet = match self
            .wallet_repo
            .find_by_user_id_and_role(seller_id, "SELLER")
            .await?
        {
            Some(wallet) => wallet,
            None => {
                return Err(ServiceError::WalletNotFound(
                    seller_id.to_string(),
                    "SELLER".to_string(),
                ));
            }
        };

        let order_id = normalized_correlation_id(order_id);
        if let Some(order_id) = order_id {
            if self
                .tx_repo
                .find_by_source_correlation_and_type(
                    ORDER_SERVICE_SOURCE,
                    order_id,
                    TransactionType::SellerEscrowSettle,
                )
                .await?
                .is_some()
            {
                return Ok(wallet);
            }
        }

        self.mutate_wallet_with_metadata(
            seller_id,
            "SELLER",
            order_id,
            Some(ORDER_SERVICE_SOURCE),
            |w| w.settle_seller_escrow(amount),
        )
        .await
    }

    pub async fn payout_to_seller(
        &self,
        seller_id: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.release_seller_payout(seller_id, amount, None).await
    }

    // ── Provisioning ─────────────────────────────────────────────

    /// Provision a wallet from an external auth event. Idempotent by event_id.
    pub async fn provision_wallet(
        &self,
        event_id: &str,
        user_id: &str,
        email: &str,
        role: &str,
        source: &str,
    ) -> Result<(), ServiceError> {
        if self.prov_repo.exists(event_id).await? {
            return Ok(());
        }

        let wallet_role = if role.eq_ignore_ascii_case("SELLER") {
            "SELLER"
        } else {
            "BUYER"
        };

        if self
            .wallet_repo
            .find_by_user_id_and_role(user_id, wallet_role)
            .await?
            .is_none()
        {
            let wallet = Wallet::new(user_id, wallet_role);
            self.wallet_repo.insert(&wallet).await?;
        }

        let now = chrono::Utc::now().to_rfc3339();
        self.prov_repo
            .insert(event_id, user_id, email, &now, source)
            .await?;

        Ok(())
    }

    pub async fn reconcile_provisioned_wallets(
        &self,
        batch_size: i64,
    ) -> Result<usize, ServiceError> {
        let limit = if batch_size <= 0 { 100 } else { batch_size };
        let events = self.prov_repo.find_recent(limit).await?;

        let mut created = 0;
        for event in &events {
            if self
                .wallet_repo
                .find_by_user_id_and_role(&event.user_id, "BUYER")
                .await?
                .is_none()
            {
                let wallet = Wallet::new(&event.user_id, "BUYER");
                self.wallet_repo.insert(&wallet).await?;
                created += 1;
            }
        }

        Ok(created)
    }

    // ── Private: Template Method (mutate) ────────────────────────

    /// Fetch → apply domain operation → persist transaction + wallet.
    ///
    /// **Template Method Pattern**: skeleton is fixed; only `operation` (the hook) varies.
    async fn mutate_wallet(
        &self,
        user_id: &str,
        role: &str,
        operation: impl FnOnce(&mut Wallet) -> Result<WalletTransaction, WalletError>,
    ) -> Result<Wallet, ServiceError> {
        let mut wallet = self.find_by_user_id_and_role(user_id, role).await?;
        let tx = operation(&mut wallet)?;
        self.tx_repo.insert(&tx).await?;
        self.wallet_repo.update(&wallet).await?;
        Ok(wallet)
    }

    async fn mutate_wallet_with_metadata(
        &self,
        user_id: &str,
        role: &str,
        correlation_id: Option<&str>,
        source_service: Option<&str>,
        operation: impl FnOnce(&mut Wallet) -> Result<WalletTransaction, WalletError>,
    ) -> Result<Wallet, ServiceError> {
        let mut wallet = self.find_by_user_id_and_role(user_id, role).await?;
        let mut tx = operation(&mut wallet)?;
        tx.correlation_id = correlation_id.map(str::to_string);
        tx.source_service = source_service.map(str::to_string);
        self.tx_repo.insert(&tx).await?;
        self.wallet_repo.update(&wallet).await?;
        Ok(wallet)
    }

    /// Records a status transaction using the Builder Pattern.
    async fn record_status_transaction(
        &self,
        user_id: &str,
        role: &str,
        tx_type: TransactionType,
        amount: Money,
        correlation_id: &str,
    ) -> Result<(), ServiceError> {
        // Builder Pattern: correlation_id and source_service set via method chaining
        let tx = WalletTransaction::builder(user_id, role, tx_type, amount)
            .correlation_id(correlation_id)
            .source_service("midtrans")
            .build();

        self.tx_repo.insert(&tx).await?;
        Ok(())
    }
}

// ── Helpers (payment expiry) ────────────────────────────────────

/// Keeps the newest TOP_UP row per Midtrans payment id (history is newest-first).
fn dedupe_top_up_history(history: Vec<WalletTransaction>) -> Vec<WalletTransaction> {
    let mut seen_top_up_correlations = std::collections::HashSet::new();
    history
        .into_iter()
        .filter(|tx| {
            if tx.transaction_type != TransactionType::TopUp {
                return true;
            }
            let Some(correlation_id) = tx.correlation_id.as_ref() else {
                return true;
            };
            seen_top_up_correlations.insert(correlation_id.clone())
        })
        .collect()
}

pub fn filter_unpaid_without_settled_top_up(
    unpaid: Vec<PaymentIntent>,
    history: &[WalletTransaction],
) -> Vec<PaymentIntent> {
    let settled: std::collections::HashSet<String> = history
        .iter()
        .filter(|tx| tx.transaction_type == TransactionType::TopUp)
        .filter_map(|tx| tx.correlation_id.clone())
        .collect();
    unpaid
        .into_iter()
        .filter(|payment| !settled.contains(&payment.id))
        .collect()
}

const PAYMENT_EXPIRY_MINUTES: i64 = 10;

pub fn payment_expires_at(created_at: &str) -> String {
    parse_payment_created_at(created_at)
        .map(|created| (created + chrono::Duration::minutes(PAYMENT_EXPIRY_MINUTES)).to_rfc3339())
        .unwrap_or_else(|| created_at.to_string())
}

fn payment_is_expired(created_at: &str) -> bool {
    parse_payment_created_at(created_at)
        .map(|created| {
            chrono::Utc::now() >= created + chrono::Duration::minutes(PAYMENT_EXPIRY_MINUTES)
        })
        .unwrap_or(false)
}

fn normalize_idempotency_key(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(128).collect())
}

fn normalized_correlation_id(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn idempotent_operation_id(prefix: &str, user_id: &str, role: &str, key: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{user_id}:{role}:{key}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{prefix}-{hash:016x}")
}

fn parse_payment_created_at(created_at: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|value| value.with_timezone(&chrono::Utc))
        .ok()
        .or_else(|| {
            chrono::DateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S%.f %:z")
                .map(|value| value.with_timezone(&chrono::Utc))
                .ok()
        })
        .or_else(|| {
            chrono::DateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S%.f%:z")
                .map(|value| value.with_timezone(&chrono::Utc))
                .ok()
        })
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|value| value.and_utc())
        })
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S%.f")
                .ok()
                .map(|value| value.and_utc())
        })
}
