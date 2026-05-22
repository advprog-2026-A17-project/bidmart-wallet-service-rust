use bidmart_wallet_service_rust::payment::{
    GatewayError, MidtransGateway, map_midtrans_transaction_status, normalize_account_number,
    normalize_bank_code,
};
use bidmart_wallet_service_rust::wallet::Money;
use std::collections::HashMap;
use std::io::Write;
use std::sync::OnceLock;
use tokio::sync::Mutex;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvGuard {
    previous: HashMap<&'static str, Option<String>>,
}

impl EnvGuard {
    fn set(vars: &[(&'static str, &'static str)]) -> Self {
        let previous = vars
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect();
        for (key, value) in vars {
            unsafe {
                std::env::set_var(key, value);
            }
        }
        Self { previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.previous {
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

async fn local_json_server(status: &str, body: &'static str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let status = status.to_string();
    listener.set_nonblocking(false).unwrap();

    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0u8; 4096];
        let _ = std::io::Read::read(&mut stream, &mut buffer);
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    format!("http://{addr}")
}

#[test]
fn normalize_bank_code_accepts_supported_bank() {
    assert_eq!(normalize_bank_code("  BCA ").unwrap(), "bca");
}

#[test]
fn gateway_error_displays_message() {
    assert_eq!(
        GatewayError("midtrans down".to_string()).to_string(),
        "midtrans down"
    );
}

#[test]
fn normalize_bank_code_rejects_unknown_bank() {
    let err = normalize_bank_code("unknown").unwrap_err();
    assert!(err.contains("unsupported bank code"));
}

#[test]
fn normalize_account_number_strips_separators() {
    assert_eq!(normalize_account_number("12 34-56").unwrap(), "123456");
}

#[test]
fn normalize_account_number_rejects_invalid() {
    assert!(normalize_account_number("12-3").is_err());
    assert!(normalize_account_number("1234ab").is_err());
}

#[test]
fn map_midtrans_transaction_status_maps_known_values() {
    assert_eq!(map_midtrans_transaction_status("capture").unwrap(), "PAID");
    assert_eq!(
        map_midtrans_transaction_status("SETTLEMENT").unwrap(),
        "PAID"
    );
    assert_eq!(map_midtrans_transaction_status("cancel").unwrap(), "FAILED");
    assert_eq!(
        map_midtrans_transaction_status("expire").unwrap(),
        "EXPIRED"
    );
    assert_eq!(
        map_midtrans_transaction_status("pending").unwrap(),
        "PENDING"
    );
}

#[test]
fn map_midtrans_transaction_status_rejects_unknown() {
    let err = map_midtrans_transaction_status("mystery").unwrap_err();
    assert!(err.contains("unknown midtrans status"));
}

#[tokio::test]
async fn create_payment_uses_local_redirect_when_server_key_missing() {
    let _lock = env_lock().lock().await;
    let server_key = std::env::var("MIDTRANS_SERVER_KEY").unwrap_or_default();
    if !server_key.is_empty() && server_key != "SB-Mid-server-local" {
        return;
    }

    let gateway = MidtransGateway::from_env();
    let page = gateway
        .create_payment("pay-123", Money::from_cents(12_500), Some("bca_va"))
        .await
        .unwrap();

    assert!(page.redirect_url.contains("/wallet?order_id=pay-123"));
    assert!(page.payment_channel.unwrap().starts_with("local-"));
    assert!(page.va_number.is_none());
}

#[tokio::test]
async fn validate_bank_account_returns_dummy_when_iris_missing() {
    let _lock = env_lock().lock().await;
    let iris_api_key = std::env::var("MIDTRANS_IRIS_API_KEY").unwrap_or_default();
    let iris_merchant_key = std::env::var("MIDTRANS_IRIS_MERCHANT_KEY").unwrap_or_default();
    let legacy_api_key = std::env::var("IRIS_API_KEY").unwrap_or_default();
    let legacy_merchant_key = std::env::var("IRIS_MERCHANT_KEY").unwrap_or_default();

    let iris_configured = !iris_api_key.is_empty()
        && !iris_merchant_key.is_empty()
        && iris_api_key != "IRIS-api-key-local"
        && iris_merchant_key != "IRIS-merchant-key-local";
    let legacy_configured = !legacy_api_key.is_empty() && !legacy_merchant_key.is_empty();

    if iris_configured || legacy_configured {
        return;
    }

    let gateway = MidtransGateway::from_env();
    let account = gateway
        .validate_bank_account("bca", "123456")
        .await
        .unwrap();

    assert_eq!(account.bank_code, "bca");
    assert_eq!(account.account_number, "123456");
    assert_eq!(account.account_name, "Validated Development Account");
}

#[tokio::test]
async fn create_payment_rejects_unknown_method() {
    let gateway = MidtransGateway::from_env();
    let err = gateway
        .create_payment("pay-999", Money::from_cents(10_000), Some("mystery"))
        .await
        .unwrap_err();

    assert!(err.0.contains("unsupported payment method"));
}

#[tokio::test]
async fn create_payout_returns_reference_when_iris_missing() {
    let _lock = env_lock().lock().await;
    let iris_api_key = std::env::var("MIDTRANS_IRIS_API_KEY").unwrap_or_default();
    let iris_merchant_key = std::env::var("MIDTRANS_IRIS_MERCHANT_KEY").unwrap_or_default();
    let legacy_api_key = std::env::var("IRIS_API_KEY").unwrap_or_default();
    let legacy_merchant_key = std::env::var("IRIS_MERCHANT_KEY").unwrap_or_default();

    let iris_configured = !iris_api_key.is_empty()
        && !iris_merchant_key.is_empty()
        && iris_api_key != "IRIS-api-key-local"
        && iris_merchant_key != "IRIS-merchant-key-local";
    let legacy_configured = !legacy_api_key.is_empty() && !legacy_merchant_key.is_empty();

    if iris_configured || legacy_configured {
        return;
    }

    let gateway = MidtransGateway::from_env();
    let result = gateway
        .create_payout(
            "user-99",
            Money::from_cents(25_00),
            &bidmart_wallet_service_rust::payment::ValidatedBankAccount {
                bank_code: "bca".to_string(),
                account_number: "123456".to_string(),
                account_name: "Dev Account".to_string(),
            },
        )
        .await
        .unwrap();

    assert!(result.reference_no.starts_with("WD-"));
}

#[tokio::test]
async fn fetch_transaction_status_requires_server_key() {
    let _lock = env_lock().lock().await;
    let server_key = std::env::var("MIDTRANS_SERVER_KEY").unwrap_or_default();
    if !server_key.is_empty() && server_key != "SB-Mid-server-local" {
        return;
    }

    let gateway = MidtransGateway::from_env();
    let err = gateway
        .fetch_transaction_status("pay-321")
        .await
        .unwrap_err();
    assert!(err.0.contains("MIDTRANS_SERVER_KEY must be configured"));
}

#[tokio::test]
async fn create_payment_uses_midtrans_charge_response_for_supported_methods() {
    let _lock = env_lock().lock().await;

    let cases = [
        (
            "bca_va",
            r#"{"transaction_status":"pending","va_numbers":[{"bank":"bca","va_number":"111"}]}"#,
            "111",
            "bca_va",
        ),
        (
            "bni_va",
            r#"{"transaction_status":"pending","va_numbers":[{"bank":"bni","va_number":"222"}]}"#,
            "222",
            "bni_va",
        ),
        (
            "bri_va",
            r#"{"transaction_status":"pending","va_numbers":[{"bank":"bri","va_number":"333"}]}"#,
            "333",
            "bri_va",
        ),
        (
            "permata_va",
            r#"{"transaction_status":"pending","permata_va_number":"444"}"#,
            "444",
            "permata_va",
        ),
        (
            "mandiri_bill",
            r#"{"transaction_status":"pending","biller_code":"70012","bill_key":"555"}"#,
            "company code 70012, bill key 555",
            "mandiri_bill",
        ),
        (
            "qris",
            r#"{"transaction_status":"pending","actions":[{"name":"generate-qr-code","url":"https://qr.local/pay"}]}"#,
            "https://qr.local/pay",
            "qris",
        ),
    ];

    for (method, response, instruction, channel) in cases {
        let url = local_json_server("200 OK", response).await;
        let _env = EnvGuard::set(&[
            ("MIDTRANS_SERVER_KEY", "server-key"),
            ("MIDTRANS_CHARGE_URL", Box::leak(url.into_boxed_str())),
        ]);

        let page = MidtransGateway::from_env()
            .create_payment("pay-1", Money::from_cents(12_500), Some(method))
            .await
            .unwrap();

        assert_eq!(page.va_number.as_deref(), Some(instruction));
        assert_eq!(page.payment_channel.as_deref(), Some(channel));
        assert!(page.redirect_url.contains("simulator.sandbox.midtrans.com"));
    }
}

#[tokio::test]
async fn create_payment_maps_midtrans_error_and_missing_instruction() {
    let _lock = env_lock().lock().await;

    let error_url = local_json_server("500 Internal Server Error", r#"{"message":"boom"}"#).await;
    let _error_env = EnvGuard::set(&[
        ("MIDTRANS_SERVER_KEY", "server-key"),
        ("MIDTRANS_CHARGE_URL", Box::leak(error_url.into_boxed_str())),
    ]);
    let err = MidtransGateway::from_env()
        .create_payment("pay-err", Money::from_cents(1000), Some("bca_va"))
        .await
        .unwrap_err();
    assert!(err.0.contains("Core API charge returned"));
    drop(_error_env);

    let missing_url = local_json_server("200 OK", r#"{"transaction_status":"pending"}"#).await;
    let _missing_env = EnvGuard::set(&[
        ("MIDTRANS_SERVER_KEY", "server-key"),
        (
            "MIDTRANS_CHARGE_URL",
            Box::leak(missing_url.into_boxed_str()),
        ),
    ]);
    let err = MidtransGateway::from_env()
        .create_payment("pay-missing", Money::from_cents(1000), Some("bca_va"))
        .await
        .unwrap_err();
    assert!(err.0.contains("payment instruction missing"));
}

#[tokio::test]
async fn fetch_transaction_status_uses_midtrans_status_api() {
    let _lock = env_lock().lock().await;

    let url = local_json_server("200 OK", r#"{"transaction_status":"settlement"}"#).await;
    let _env = EnvGuard::set(&[
        ("MIDTRANS_SERVER_KEY", "server-key"),
        ("MIDTRANS_STATUS_BASE_URL", Box::leak(url.into_boxed_str())),
    ]);

    let status = MidtransGateway::from_env()
        .fetch_transaction_status("pay-1")
        .await
        .unwrap();

    assert_eq!(status, "settlement");
}

#[tokio::test]
async fn validate_bank_account_uses_iris_response() {
    let _lock = env_lock().lock().await;

    let url = local_json_server(
        "200 OK",
        r#"{"account_name":"Jane Buyer","account_no":"123456","bank":"bca"}"#,
    )
    .await;
    let _env = EnvGuard::set(&[
        ("MIDTRANS_IRIS_API_KEY", "iris-key"),
        ("MIDTRANS_IRIS_MERCHANT_KEY", "merchant-key"),
        ("MIDTRANS_IRIS_BASE_URL", Box::leak(url.into_boxed_str())),
    ]);

    let account = MidtransGateway::from_env()
        .validate_bank_account("bca", "123456")
        .await
        .unwrap();

    assert_eq!(account.account_name, "Jane Buyer");
    assert_eq!(account.account_number, "123456");
    assert_eq!(account.bank_code, "bca");
}

#[tokio::test]
async fn validate_bank_account_rejects_iris_response_without_name() {
    let _lock = env_lock().lock().await;

    let url = local_json_server("200 OK", r#"{"account_no":"123456","bank":"bca"}"#).await;
    let _env = EnvGuard::set(&[
        ("MIDTRANS_IRIS_API_KEY", "iris-key"),
        ("MIDTRANS_IRIS_MERCHANT_KEY", "merchant-key"),
        ("MIDTRANS_IRIS_BASE_URL", Box::leak(url.into_boxed_str())),
    ]);

    let err = MidtransGateway::from_env()
        .validate_bank_account("bca", "123456")
        .await
        .unwrap_err();

    assert!(err.0.contains("did not return account name"));
}

#[tokio::test]
async fn create_payout_uses_iris_response_reference() {
    let _lock = env_lock().lock().await;

    let url = local_json_server("200 OK", r#"{"payouts":[{"reference_no":"WD-remote"}]}"#).await;
    let _env = EnvGuard::set(&[
        ("MIDTRANS_IRIS_API_KEY", "iris-key"),
        ("MIDTRANS_IRIS_MERCHANT_KEY", "merchant-key"),
        ("MIDTRANS_IRIS_BASE_URL", Box::leak(url.into_boxed_str())),
    ]);

    let result = MidtransGateway::from_env()
        .create_payout(
            "user-1",
            Money::from_cents(2500),
            &bidmart_wallet_service_rust::payment::ValidatedBankAccount {
                bank_code: "bca".to_string(),
                account_number: "123456".to_string(),
                account_name: "Jane Buyer".to_string(),
            },
        )
        .await
        .unwrap();

    assert_eq!(result.reference_no, "WD-remote");
}
