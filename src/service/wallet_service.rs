use sqlx::SqlitePool;

use crate::persistence::repositories::{
    ProvisioningEventRepository, TransactionRepository, WalletRepository,
};
use crate::wallet::{
    Hold, Money, PaymentIntent, Wallet, WalletError, WalletTransaction, WalletWithdrawal,
};

// ── ServiceError ────────────────────────────────────────────────

/// Service-level error combining domain and persistence failures.
#[derive(Debug)]
pub enum ServiceError {
    WalletNotFound(String),
    Domain(WalletError),
    Persistence(sqlx::Error),
    TransactionNotFound(String),
    ForbiddenAccess,
    HoldFailed(String),
    InvalidPaymentStatus(String),
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

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WalletNotFound(uid) => write!(f, "wallet not found for user {uid}"),
            Self::Domain(e) => write!(f, "{e}"),
            Self::Persistence(e) => write!(f, "persistence error: {e}"),
            Self::TransactionNotFound(id) => write!(f, "transaction not found: {id}"),
            Self::ForbiddenAccess => write!(f, "forbidden transaction access"),
            Self::HoldFailed(msg) => write!(f, "hold operation failed: {msg}"),
            Self::InvalidPaymentStatus(status) => write!(f, "invalid payment status: {status}"),
        }
    }
}

// ── WalletService ───────────────────────────────────────────────

/// Orchestrates wallet use cases by coordinating domain logic and persistence.
///
/// Follows the Service Layer pattern — controllers call into this layer,
/// which fetches domain entities, invokes domain operations, and persists
/// the resulting state and transaction records.
pub struct WalletService {
    wallet_repo: WalletRepository,
    tx_repo: TransactionRepository,
    prov_repo: ProvisioningEventRepository,
}

impl WalletService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            wallet_repo: WalletRepository::new(pool.clone()),
            tx_repo: TransactionRepository::new(pool.clone()),
            prov_repo: ProvisioningEventRepository::new(pool),
        }
    }

    // ── Queries ──────────────────────────────────────────────────

    pub async fn find_by_user_id(&self, user_id: &str) -> Result<Wallet, ServiceError> {
        self.wallet_repo
            .find_by_user_id(user_id)
            .await?
            .ok_or_else(|| ServiceError::WalletNotFound(user_id.to_string()))
    }

    pub async fn find_all(&self) -> Result<Vec<Wallet>, ServiceError> {
        Ok(self.wallet_repo.find_all().await?)
    }

    pub async fn get_transaction_history(
        &self,
        user_id: &str,
    ) -> Result<Vec<WalletTransaction>, ServiceError> {
        Ok(self.tx_repo.find_by_user_id(user_id).await?)
    }

    // ── Commands ─────────────────────────────────────────────────

    pub async fn create_wallet(&self, user_id: &str) -> Result<Wallet, ServiceError> {
        let wallet = Wallet::new(user_id);
        self.wallet_repo.insert(&wallet).await?;
        Ok(wallet)
    }

    pub async fn top_up(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, |w| w.top_up(amount)).await
    }

    pub async fn withdraw(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, |w| w.withdraw(amount)).await
    }

    pub async fn create_top_up_intent(
        &self,
        user_id: &str,
        amount: Money,
    ) -> Result<PaymentIntent, ServiceError> {
        if amount.is_zero() {
            return Err(ServiceError::Domain(WalletError::InvalidAmount));
        }
        self.find_by_user_id(user_id).await?;
        Ok(self.wallet_repo.insert_payment_intent(user_id, amount).await?)
    }

    pub async fn simulate_payment_status(
        &self,
        payment_id: &str,
        status: &str,
    ) -> Result<PaymentIntent, ServiceError> {
        let normalized = status.to_ascii_uppercase();
        if !matches!(normalized.as_str(), "PAID" | "FAILED" | "EXPIRED") {
            return Err(ServiceError::InvalidPaymentStatus(status.to_string()));
        }

        let payment = self
            .wallet_repo
            .find_payment_intent(payment_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(payment_id.to_string()))?;

        if payment.status == "PENDING" {
            if normalized == "PAID" {
                self.top_up(&payment.user_id, Money::from_cents(payment.amount_cents as u64))
                    .await?;
            }
            self.wallet_repo
                .update_payment_status(payment_id, &normalized)
                .await?;
        }

        self.wallet_repo
            .find_payment_intent(payment_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(payment_id.to_string()))
    }

    pub async fn create_withdrawal(
        &self,
        user_id: &str,
        amount: Money,
        bank_account: &str,
    ) -> Result<WalletWithdrawal, ServiceError> {
        if bank_account.trim().is_empty() {
            return Err(ServiceError::InvalidPaymentStatus("missing bank account".to_string()));
        }
        self.withdraw(user_id, amount).await?;
        Ok(self
            .wallet_repo
            .insert_withdrawal(user_id, amount, bank_account)
            .await?)
    }

    pub async fn simulate_withdrawal_status(
        &self,
        withdrawal_id: &str,
        status: &str,
    ) -> Result<WalletWithdrawal, ServiceError> {
        let normalized = status.to_ascii_uppercase();
        if !matches!(normalized.as_str(), "COMPLETED" | "FAILED") {
            return Err(ServiceError::InvalidPaymentStatus(status.to_string()));
        }

        let withdrawal = self
            .wallet_repo
            .find_withdrawal(withdrawal_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(withdrawal_id.to_string()))?;

        if withdrawal.status == "PENDING" {
            if normalized == "FAILED" {
                self.top_up(
                    &withdrawal.user_id,
                    Money::from_cents(withdrawal.amount_cents as u64),
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

    pub async fn hold(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, |w| w.hold(amount)).await
    }

    pub async fn release(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, |w| w.release(amount)).await
    }

    pub async fn convert(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, |w| w.convert(amount)).await
    }

    pub async fn bid(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        self.mutate_wallet(user_id, |w| w.bid(amount)).await
    }

    pub async fn cancel_bid(&self, user_id: &str, bid_tx_id: &str) -> Result<(), ServiceError> {
        let tx = self
            .tx_repo
            .find_by_id(bid_tx_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(bid_tx_id.to_string()))?;

        if tx.user_id != user_id {
            return Err(ServiceError::ForbiddenAccess);
        }

        self.mutate_wallet(user_id, |w| w.release(tx.amount)).await?;
        Ok(())
    }

    pub async fn hold_funds(
        &self,
        user_id: &str,
        auction_id: &str,
        bid_id: &str,
        amount: Money,
        hold_id: &str,
        expires_at: &str,
    ) -> Result<Hold, ServiceError> {
        // Cari dompet berdasarkan user_id terlebih dahulu
        let wallet = self.find_by_user_id(user_id).await?;

        // Teruskan data ke repository yang sudah dilengkapi database transaction
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

    // ── Provisioning ─────────────────────────────────────────────

    /// Provision a wallet from an external auth event. Idempotent by event_id.
    pub async fn provision_wallet(
        &self,
        event_id: &str,
        user_id: &str,
        email: &str,
        source: &str,
    ) -> Result<(), ServiceError> {
        // Idempotency: skip if this event was already processed
        if self.prov_repo.exists(event_id).await? {
            return Ok(());
        }

        // Only create a wallet if the user doesn't already have one
        if self.wallet_repo.find_by_user_id(user_id).await?.is_none() {
            let wallet = Wallet::new(user_id);
            self.wallet_repo.insert(&wallet).await?;
        }

        // Record the event
        let now = chrono::Utc::now().to_rfc3339();
        self.prov_repo
            .insert(event_id, user_id, email, &now, source)
            .await?;

        Ok(())
    }

    /// Reconcile provisioning events by creating wallets for any
    /// provisioned users that don't yet have one.
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
                .find_by_user_id(&event.user_id)
                .await?
                .is_none()
            {
                let wallet = Wallet::new(&event.user_id);
                self.wallet_repo.insert(&wallet).await?;
                created += 1;
            }
        }

        Ok(created)
    }

    // ── Private: DRY mutation helper ─────────────────────────────

    /// Fetch → apply domain operation → persist transaction + wallet.
    async fn mutate_wallet(
        &self,
        user_id: &str,
        operation: impl FnOnce(&mut Wallet) -> Result<WalletTransaction, WalletError>,
    ) -> Result<Wallet, ServiceError> {
        let mut wallet = self.find_by_user_id(user_id).await?;
        let tx = operation(&mut wallet)?;
        self.tx_repo.insert(&tx).await?;
        self.wallet_repo.update(&wallet).await?;
        Ok(wallet)
    }
}
