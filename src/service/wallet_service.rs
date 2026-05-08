use sqlx::AnyPool;

use crate::persistence::repositories::{
    ProvisioningEventRepository, TransactionRepository, WalletRepository,
};
use crate::wallet::{
    Hold, Money, PaymentIntent, TransactionType, Wallet, WalletError, WalletTransaction,
    WalletWithdrawal,
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
            Self::Midtrans(message) => write!(f, "midtrans error: {message}"),
        }
    }
}

#[derive(Debug, Clone)]
struct MidtransPaymentPage {
    redirect_url: String,
    va_number: Option<String>,
    payment_channel: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct MidtransBankTransferChargeResponse {
    transaction_status: Option<String>,
    va_numbers: Option<Vec<MidtransVaNumber>>,
    permata_va_number: Option<String>,
    biller_code: Option<String>,
    bill_key: Option<String>,
    actions: Option<Vec<MidtransAction>>,
}

#[derive(Debug, serde::Deserialize)]
struct MidtransVaNumber {
    bank: String,
    va_number: String,
}

#[derive(Debug, serde::Deserialize)]
struct MidtransAction {
    name: String,
    url: String,
}

#[derive(Debug, serde::Deserialize)]
struct MidtransTransactionStatusResponse {
    transaction_status: String,
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
    pub fn new(pool: AnyPool) -> Self {
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
        payment_method: Option<&str>,
    ) -> Result<PaymentIntent, ServiceError> {
        if amount.is_zero() {
            return Err(ServiceError::Domain(WalletError::InvalidAmount));
        }
        self.find_by_user_id(user_id).await?;
        let payment_id = uuid::Uuid::new_v4().to_string();
        let payment_page = create_midtrans_payment(&payment_id, amount, payment_method).await?;
        Ok(self
            .wallet_repo
            .insert_payment_intent(
                &payment_id,
                user_id,
                amount,
                &payment_page.redirect_url,
                payment_page.va_number.as_deref(),
                payment_page.payment_channel.as_deref(),
            )
            .await?)
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
        let normalized = map_midtrans_transaction_status(transaction_status)?;
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

        if payment.status != "PENDING" {
            return Ok(payment);
        }

        let transaction_status = fetch_midtrans_transaction_status(payment_id).await?;
        let normalized = map_midtrans_transaction_status(&transaction_status)?;
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

        let payment = self
            .wallet_repo
            .find_payment_intent(payment_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(payment_id.to_string()))?;

        if payment.status == "PENDING" && normalized != "PENDING" {
            match normalized {
                "PAID" => {
                    self.top_up(
                        &payment.user_id,
                        Money::from_cents(payment.amount_cents as u64),
                    )
                    .await?;
                }
                "FAILED" => {
                    self.record_status_transaction(
                        &payment.user_id,
                        TransactionType::TopUpFailed,
                        Money::from_cents(payment.amount_cents as u64),
                        payment_id,
                    )
                    .await?;
                }
                "EXPIRED" => {
                    self.record_status_transaction(
                        &payment.user_id,
                        TransactionType::TopUpExpired,
                        Money::from_cents(payment.amount_cents as u64),
                        payment_id,
                    )
                    .await?;
                }
                _ => {}
            }
            self.wallet_repo
                .update_payment_status(payment_id, normalized)
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
            return Err(ServiceError::InvalidPaymentStatus(
                "missing bank account".to_string(),
            ));
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
                let amount = Money::from_cents(withdrawal.amount_cents as u64);
                self.top_up(&withdrawal.user_id, amount).await?;
                let tx_type = if normalized == "FAILED" {
                    TransactionType::WithdrawFailed
                } else {
                    TransactionType::WithdrawExpired
                };
                self.record_status_transaction(&withdrawal.user_id, tx_type, amount, withdrawal_id)
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

        self.mutate_wallet(user_id, |w| w.release(tx.amount))
            .await?;
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

    async fn record_status_transaction(
        &self,
        user_id: &str,
        tx_type: TransactionType,
        amount: Money,
        correlation_id: &str,
    ) -> Result<(), ServiceError> {
        let mut tx = WalletTransaction::new(user_id, tx_type, amount);
        tx.correlation_id = Some(correlation_id.to_string());
        tx.source_service = Some("midtrans".to_string());
        self.tx_repo.insert(&tx).await?;
        Ok(())
    }
}

fn map_midtrans_transaction_status(status: &str) -> Result<String, ServiceError> {
    let normalized = status.to_ascii_lowercase();
    match normalized.as_str() {
        "capture" | "settlement" => Ok("PAID".to_string()),
        "deny" | "cancel" | "failure" => Ok("FAILED".to_string()),
        "expire" => Ok("EXPIRED".to_string()),
        "pending" => Ok("PENDING".to_string()),
        other => Err(ServiceError::InvalidPaymentStatus(other.to_string())),
    }
}

#[derive(Debug, Clone, Copy)]
enum MidtransPaymentMethod {
    BcaVa,
    BniVa,
    BriVa,
    PermataVa,
    MandiriBill,
    Qris,
}

impl MidtransPaymentMethod {
    fn parse(value: Option<&str>) -> Result<Self, ServiceError> {
        match value.unwrap_or("bca_va").trim().to_ascii_lowercase().as_str() {
            "bca" | "bca_va" => Ok(Self::BcaVa),
            "bni" | "bni_va" => Ok(Self::BniVa),
            "bri" | "bri_va" => Ok(Self::BriVa),
            "permata" | "permata_va" => Ok(Self::PermataVa),
            "mandiri" | "mandiri_bill" => Ok(Self::MandiriBill),
            "qris" => Ok(Self::Qris),
            other => Err(ServiceError::InvalidPaymentStatus(format!(
                "unsupported payment method: {other}"
            ))),
        }
    }

    fn channel(self) -> &'static str {
        match self {
            Self::BcaVa => "bca_va",
            Self::BniVa => "bni_va",
            Self::BriVa => "bri_va",
            Self::PermataVa => "permata_va",
            Self::MandiriBill => "mandiri_bill",
            Self::Qris => "qris",
        }
    }

    fn charge_request(self, payment_id: &str, amount: Money) -> serde_json::Value {
        let transaction_details = serde_json::json!({
            "order_id": payment_id,
            "gross_amount": amount.cents() as i64
        });

        match self {
            Self::BcaVa => serde_json::json!({
                "payment_type": "bank_transfer",
                "transaction_details": transaction_details,
                "bank_transfer": { "bank": "bca" }
            }),
            Self::BniVa => serde_json::json!({
                "payment_type": "bank_transfer",
                "transaction_details": transaction_details,
                "bank_transfer": { "bank": "bni" }
            }),
            Self::BriVa => serde_json::json!({
                "payment_type": "bank_transfer",
                "transaction_details": transaction_details,
                "bank_transfer": { "bank": "bri" }
            }),
            Self::PermataVa => serde_json::json!({
                "payment_type": "permata",
                "transaction_details": transaction_details
            }),
            Self::MandiriBill => serde_json::json!({
                "payment_type": "echannel",
                "transaction_details": transaction_details,
                "echannel": {
                    "bill_info1": "Payment:",
                    "bill_info2": "BidMart top up"
                }
            }),
            Self::Qris => serde_json::json!({
                "payment_type": "qris",
                "transaction_details": transaction_details
            }),
        }
    }

    fn payment_page(self, charge: MidtransBankTransferChargeResponse) -> Result<MidtransPaymentPage, ServiceError> {
        let _ = charge.transaction_status.as_deref();
        let simulator = simulator_url_for(self);
        let instruction = match self {
            Self::BcaVa | Self::BniVa | Self::BriVa => charge
                .va_numbers
                .unwrap_or_default()
                .into_iter()
                .find(|va| va.bank.eq_ignore_ascii_case(bank_name_for(self)))
                .map(|va| va.va_number),
            Self::PermataVa => charge.permata_va_number,
            Self::MandiriBill => match (charge.biller_code, charge.bill_key) {
                (Some(code), Some(key)) => Some(format!("company code {code}, bill key {key}")),
                _ => None,
            },
            Self::Qris => charge
                .actions
                .unwrap_or_default()
                .into_iter()
                .find(|action| action.name == "generate-qr-code")
                .map(|action| action.url),
        }
        .ok_or_else(|| {
            ServiceError::Midtrans(format!(
                "{} payment instruction missing from charge response",
                self.channel()
            ))
        })?;

        Ok(MidtransPaymentPage {
            redirect_url: simulator,
            va_number: Some(instruction),
            payment_channel: Some(self.channel().to_string()),
        })
    }
}

fn bank_name_for(method: MidtransPaymentMethod) -> &'static str {
    match method {
        MidtransPaymentMethod::BcaVa => "bca",
        MidtransPaymentMethod::BniVa => "bni",
        MidtransPaymentMethod::BriVa => "bri",
        _ => "",
    }
}

fn simulator_url_for(method: MidtransPaymentMethod) -> String {
    let key = match method {
        MidtransPaymentMethod::BcaVa => "MIDTRANS_BCA_VA_SIMULATOR_URL",
        MidtransPaymentMethod::BniVa => "MIDTRANS_BNI_VA_SIMULATOR_URL",
        MidtransPaymentMethod::BriVa => "MIDTRANS_BRI_VA_SIMULATOR_URL",
        MidtransPaymentMethod::PermataVa => "MIDTRANS_PERMATA_VA_SIMULATOR_URL",
        MidtransPaymentMethod::MandiriBill => "MIDTRANS_MANDIRI_BILL_SIMULATOR_URL",
        MidtransPaymentMethod::Qris => "MIDTRANS_QRIS_SIMULATOR_URL",
    };
    std::env::var(key).unwrap_or_else(|_| match method {
        MidtransPaymentMethod::BcaVa => "https://simulator.sandbox.midtrans.com/bca/va/index".to_string(),
        MidtransPaymentMethod::BniVa => "https://simulator.sandbox.midtrans.com/bni/va/index".to_string(),
        MidtransPaymentMethod::BriVa => "https://simulator.sandbox.midtrans.com/bri/va/index".to_string(),
        MidtransPaymentMethod::PermataVa => "https://simulator.sandbox.midtrans.com/openapi/va/index".to_string(),
        MidtransPaymentMethod::MandiriBill => "https://simulator.sandbox.midtrans.com/openapi/mandiri/index".to_string(),
        MidtransPaymentMethod::Qris => "https://simulator.sandbox.midtrans.com/qris/index".to_string(),
    })
}

async fn create_midtrans_payment(
    payment_id: &str,
    amount: Money,
    payment_method: Option<&str>,
) -> Result<MidtransPaymentPage, ServiceError> {
    let charge_url = std::env::var("MIDTRANS_CHARGE_URL")
        .unwrap_or_else(|_| "https://api.sandbox.midtrans.com/v2/charge".to_string());
    let server_key = std::env::var("MIDTRANS_SERVER_KEY").unwrap_or_default();
    let method = MidtransPaymentMethod::parse(payment_method)?;

    let frontend_base_url =
        std::env::var("FRONTEND_BASE_URL").unwrap_or_else(|_| "http://localhost".to_string());

    if server_key.is_empty() || server_key == "SB-Mid-server-local" {
        let wallet_url = format!("{}/wallet", frontend_base_url.trim_end_matches('/'));
        return Ok(MidtransPaymentPage {
            redirect_url: format!(
                "{wallet_url}?order_id={payment_id}&transaction_status=settlement&status_code=200"
            ),
            va_number: None,
            payment_channel: Some(format!("local-{}", method.channel())),
        });
    }

    let request = method.charge_request(payment_id, amount);

    let response = reqwest::Client::new()
        .post(&charge_url)
        .basic_auth(server_key, Some(""))
        .json(&request)
        .send()
        .await
        .map_err(|error| ServiceError::Midtrans(error.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(ServiceError::Midtrans(format!(
            "Core API charge returned {status}: {body}"
        )));
    }

    let charge = response
        .json::<MidtransBankTransferChargeResponse>()
        .await
        .map_err(|error| ServiceError::Midtrans(error.to_string()))?;

    method.payment_page(charge)
}

async fn fetch_midtrans_transaction_status(payment_id: &str) -> Result<String, ServiceError> {
    let status_base_url = std::env::var("MIDTRANS_STATUS_BASE_URL")
        .unwrap_or_else(|_| "https://api.sandbox.midtrans.com/v2".to_string());
    let server_key = std::env::var("MIDTRANS_SERVER_KEY").unwrap_or_default();

    if server_key.is_empty() || server_key == "SB-Mid-server-local" {
        return Err(ServiceError::Midtrans(
            "MIDTRANS_SERVER_KEY must be configured to sync Midtrans sandbox status".to_string(),
        ));
    }

    let response = reqwest::Client::new()
        .get(format!("{}/{}/status", status_base_url.trim_end_matches('/'), payment_id))
        .basic_auth(server_key, Some(""))
        .send()
        .await
        .map_err(|error| ServiceError::Midtrans(error.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(ServiceError::Midtrans(format!(
            "Transaction status API returned {status}: {body}"
        )));
    }

    let status = response
        .json::<MidtransTransactionStatusResponse>()
        .await
        .map_err(|error| ServiceError::Midtrans(error.to_string()))?;
    Ok(status.transaction_status)
}
