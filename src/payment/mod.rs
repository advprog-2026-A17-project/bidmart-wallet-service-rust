/// Payment module — Facade over the Midtrans Core API and IRIS Disbursement subsystems.

// ── Public types used by WalletService ──────────────────────────────

/// Payment page details returned after creating a Midtrans charge.
#[derive(Debug, Clone)]
pub struct PaymentPage {
    pub redirect_url: String,
    pub va_number: Option<String>,
    pub payment_channel: Option<String>,
}

/// A bank account that has been validated by Midtrans IRIS.
#[derive(Debug, Clone)]
pub struct ValidatedBankAccount {
    pub bank_code: String,
    pub account_number: String,
    pub account_name: String,
}

/// Disbursement result — the reference number returned by IRIS.
#[derive(Debug, Clone)]
pub struct PayoutResult {
    pub reference_no: String,
}

/// Gateway error — kept separate from `ServiceError` for flexible mapping.
#[derive(Debug)]
pub struct GatewayError(pub String);

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── MidtransGateway (Facade) ─────────────────────────────────────

/// **Facade Pattern**: hides Midtrans Core API + IRIS complexity behind a simple interface.
pub struct MidtransGateway;

impl MidtransGateway {
    /// Creates a gateway instance — reads configuration from env vars.
    pub fn from_env() -> Self {
        Self
    }

    // ── Public facade methods ────────────────────────────────────

    /// Creates a Midtrans charge and returns the payment page details.
    pub async fn create_payment(
        &self,
        payment_id: &str,
        amount: crate::wallet::Money,
        payment_method: Option<&str>,
    ) -> Result<PaymentPage, GatewayError> {
        let method = MidtransPaymentMethod::parse(payment_method)
            .map_err(|e| GatewayError(e))?;
        create_midtrans_payment_inner(payment_id, amount, method).await
    }

    /// Validates a bank account via IRIS. Returns a dummy account in local environments.
    pub async fn validate_bank_account(
        &self,
        bank_code: &str,
        account_number: &str,
    ) -> Result<ValidatedBankAccount, GatewayError> {
        validate_midtrans_bank_account(bank_code, account_number).await
    }

    /// Sends a disbursement (payout) to the beneficiary account via IRIS.
    pub async fn create_payout(
        &self,
        user_id: &str,
        amount: crate::wallet::Money,
        account: &ValidatedBankAccount,
    ) -> Result<PayoutResult, GatewayError> {
        create_midtrans_payout(user_id, amount, account).await
    }

    /// Fetches the transaction status from Midtrans. Requires `MIDTRANS_SERVER_KEY`.
    pub async fn fetch_transaction_status(
        &self,
        payment_id: &str,
    ) -> Result<String, GatewayError> {
        fetch_midtrans_transaction_status(payment_id).await
    }
}

// ── Internal helpers (private) ───────────────────────────────────

const SUPPORTED_IRIS_BANKS: &[&str] = &[
    "bca", "bni", "bri", "mandiri", "permata", "cimb", "danamon", "bsi", "btn", "ocbc", "panin",
];

pub fn normalize_bank_code(bank_code: &str) -> Result<String, String> {
    let normalized = bank_code.trim().to_ascii_lowercase();
    if normalized.is_empty() || !SUPPORTED_IRIS_BANKS.contains(&normalized.as_str()) {
        return Err(format!("unsupported bank code: {bank_code}"));
    }
    Ok(normalized)
}

pub fn normalize_account_number(account_number: &str) -> Result<String, String> {
    let normalized: String = account_number
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();

    if normalized.len() < 6
        || normalized.len() > 32
        || !normalized.chars().all(|c| c.is_ascii_digit())
    {
        return Err("invalid bank account number".to_string());
    }
    Ok(normalized)
}

pub fn map_midtrans_transaction_status(status: &str) -> Result<String, String> {
    let normalized = status.to_ascii_lowercase();
    match normalized.as_str() {
        "capture" | "settlement" => Ok("PAID".to_string()),
        "deny" | "cancel" | "failure" => Ok("FAILED".to_string()),
        "expire" => Ok("EXPIRED".to_string()),
        "pending" => Ok("PENDING".to_string()),
        other => Err(format!("unknown midtrans status: {other}")),
    }
}

fn iris_credentials() -> Option<(String, String)> {
    let api_key = std::env::var("MIDTRANS_IRIS_API_KEY")
        .or_else(|_| std::env::var("IRIS_API_KEY"))
        .unwrap_or_default();
    let merchant_key = std::env::var("MIDTRANS_IRIS_MERCHANT_KEY")
        .or_else(|_| std::env::var("IRIS_MERCHANT_KEY"))
        .unwrap_or_default();

    if api_key.is_empty()
        || merchant_key.is_empty()
        || api_key == "IRIS-api-key-local"
        || merchant_key == "IRIS-merchant-key-local"
    {
        None
    } else {
        Some((api_key, merchant_key))
    }
}

fn iris_base_url() -> String {
    std::env::var("MIDTRANS_IRIS_BASE_URL")
        .or_else(|_| std::env::var("IRIS_BASE_URL"))
        .unwrap_or_else(|_| "https://app.sandbox.midtrans.com/iris/api/v1".to_string())
}

// ── Internal types (Midtrans responses) ──────────────────────────────

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

#[derive(Debug, serde::Deserialize)]
struct MidtransAccountValidationResponse {
    account_name: Option<String>,
    account_no: Option<String>,
    bank: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct MidtransPayoutResponse {
    payouts: Option<Vec<MidtransPayoutItem>>,
    reference_no: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct MidtransPayoutItem {
    reference_no: Option<String>,
}

// ── Strategy: MidtransPaymentMethod ─────────────────────────────
// (Moved here as it is part of the Midtrans Facade subsystem)

/// Strategy Pattern for Midtrans payment methods — each variant encapsulates
/// its own `charge_request` format and `payment_page` extraction logic.
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
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value
            .unwrap_or("bca_va")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "bca" | "bca_va" => Ok(Self::BcaVa),
            "bni" | "bni_va" => Ok(Self::BniVa),
            "bri" | "bri_va" => Ok(Self::BriVa),
            "permata" | "permata_va" => Ok(Self::PermataVa),
            "mandiri" | "mandiri_bill" => Ok(Self::MandiriBill),
            "qris" => Ok(Self::Qris),
            other => Err(format!("unsupported payment method: {other}")),
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

    fn charge_request(self, payment_id: &str, amount: crate::wallet::Money) -> serde_json::Value {
        let transaction_details = serde_json::json!({
            "order_id": payment_id,
            "gross_amount": (amount.cents()/100) as i64
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

    fn payment_page(
        self,
        charge: MidtransBankTransferChargeResponse,
    ) -> Result<PaymentPage, GatewayError> {
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
            GatewayError(format!(
                "{} payment instruction missing from charge response",
                self.channel()
            ))
        })?;

        Ok(PaymentPage {
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
        MidtransPaymentMethod::BcaVa => {
            "https://simulator.sandbox.midtrans.com/bca/va/index".to_string()
        }
        MidtransPaymentMethod::BniVa => {
            "https://simulator.sandbox.midtrans.com/bni/va/index".to_string()
        }
        MidtransPaymentMethod::BriVa => {
            "https://simulator.sandbox.midtrans.com/bri/va/index".to_string()
        }
        MidtransPaymentMethod::PermataVa => {
            "https://simulator.sandbox.midtrans.com/openapi/va/index".to_string()
        }
        MidtransPaymentMethod::MandiriBill => {
            "https://simulator.sandbox.midtrans.com/openapi/mandiri/index".to_string()
        }
        MidtransPaymentMethod::Qris => {
            "https://simulator.sandbox.midtrans.com/qris/index".to_string()
        }
    })
}

// ── Internal async function implementations ──────────────────────────

async fn create_midtrans_payment_inner(
    payment_id: &str,
    amount: crate::wallet::Money,
    method: MidtransPaymentMethod,
) -> Result<PaymentPage, GatewayError> {
    let charge_url = std::env::var("MIDTRANS_CHARGE_URL")
        .unwrap_or_else(|_| "https://api.sandbox.midtrans.com/v2/charge".to_string());
    let server_key = std::env::var("MIDTRANS_SERVER_KEY").unwrap_or_default();

    let frontend_base_url =
        std::env::var("FRONTEND_BASE_URL").unwrap_or_else(|_| "http://localhost".to_string());

    if server_key.is_empty() || server_key == "SB-Mid-server-local" {
        let wallet_url = format!("{}/wallet", frontend_base_url.trim_end_matches('/'));
        return Ok(PaymentPage {
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
        .map_err(|e| GatewayError(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(GatewayError(format!(
            "Core API charge returned {status}: {body}"
        )));
    }

    let charge = response
        .json::<MidtransBankTransferChargeResponse>()
        .await
        .map_err(|e| GatewayError(e.to_string()))?;

    method.payment_page(charge)
}

async fn validate_midtrans_bank_account(
    bank_code: &str,
    account_number: &str,
) -> Result<ValidatedBankAccount, GatewayError> {
    let Some((api_key, _merchant_key)) = iris_credentials() else {
        return Ok(ValidatedBankAccount {
            bank_code: bank_code.to_string(),
            account_number: account_number.to_string(),
            account_name: "Validated Development Account".to_string(),
        });
    };

    let response = reqwest::Client::new()
        .get(format!(
            "{}/account_validation",
            iris_base_url().trim_end_matches('/')
        ))
        .basic_auth(api_key, Some(""))
        .query(&[("bank", bank_code), ("account", account_number)])
        .send()
        .await
        .map_err(|e| GatewayError(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(GatewayError(format!(
            "IRIS account validation returned {status}: {body}"
        )));
    }

    let validation = response
        .json::<MidtransAccountValidationResponse>()
        .await
        .map_err(|e| GatewayError(e.to_string()))?;

    let account_name = validation
        .account_name
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            GatewayError("IRIS account validation did not return account name".to_string())
        })?;

    Ok(ValidatedBankAccount {
        bank_code: validation.bank.unwrap_or_else(|| bank_code.to_string()),
        account_number: validation
            .account_no
            .unwrap_or_else(|| account_number.to_string()),
        account_name,
    })
}

async fn create_midtrans_payout(
    user_id: &str,
    amount: crate::wallet::Money,
    account: &ValidatedBankAccount,
) -> Result<PayoutResult, GatewayError> {
    let reference_no = format!("WD-{}", uuid::Uuid::new_v4());
    let Some((api_key, _merchant_key)) = iris_credentials() else {
        return Ok(PayoutResult { reference_no });
    };

    let body = serde_json::json!({
        "payouts": [{
            "beneficiary_name": account.account_name,
            "beneficiary_account": account.account_number,
            "beneficiary_bank": account.bank_code,
            "beneficiary_email": format!("{user_id}@bidmart.local"),
            "amount": amount.cents(),
            "notes": "BidMart wallet withdrawal",
            "reference_no": reference_no
        }]
    });

    let response = reqwest::Client::new()
        .post(format!("{}/payouts", iris_base_url().trim_end_matches('/')))
        .basic_auth(api_key, Some(""))
        .json(&body)
        .send()
        .await
        .map_err(|e| GatewayError(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(GatewayError(format!(
            "IRIS payout creation returned {status}: {body}"
        )));
    }

    let payout = response
        .json::<MidtransPayoutResponse>()
        .await
        .map_err(|e| GatewayError(e.to_string()))?;

    let response_reference = payout
        .payouts
        .and_then(|mut payouts| payouts.pop())
        .and_then(|item| item.reference_no)
        .or(payout.reference_no)
        .unwrap_or(reference_no);

    Ok(PayoutResult {
        reference_no: response_reference,
    })
}

async fn fetch_midtrans_transaction_status(payment_id: &str) -> Result<String, GatewayError> {
    let status_base_url = std::env::var("MIDTRANS_STATUS_BASE_URL")
        .unwrap_or_else(|_| "https://api.sandbox.midtrans.com/v2".to_string());
    let server_key = std::env::var("MIDTRANS_SERVER_KEY").unwrap_or_default();

    if server_key.is_empty() || server_key == "SB-Mid-server-local" {
        return Err(GatewayError(
            "MIDTRANS_SERVER_KEY must be configured to sync Midtrans sandbox status".to_string(),
        ));
    }

    let response = reqwest::Client::new()
        .get(format!(
            "{}/{}/status",
            status_base_url.trim_end_matches('/'),
            payment_id
        ))
        .basic_auth(server_key, Some(""))
        .send()
        .await
        .map_err(|e| GatewayError(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(GatewayError(format!(
            "Transaction status API returned {status}: {body}"
        )));
    }

    let status = response
        .json::<MidtransTransactionStatusResponse>()
        .await
        .map_err(|e| GatewayError(e.to_string()))?;

    Ok(status.transaction_status)
}
