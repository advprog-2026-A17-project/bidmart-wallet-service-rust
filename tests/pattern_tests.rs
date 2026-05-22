use bidmart_wallet_service_rust::payment::{
    GatewayError, PaymentPage, PayoutResult, ValidatedBankAccount,
};
use bidmart_wallet_service_rust::server::{connect_pool, run_migrations};
use bidmart_wallet_service_rust::service::payment_strategy::{
    PaymentGateway, PaymentStatusStrategy,
};
use bidmart_wallet_service_rust::service::wallet_service::WalletService;
use bidmart_wallet_service_rust::service::wallet_validation::{
    WalletCommandContext, WalletValidationChain,
};
use bidmart_wallet_service_rust::wallet::Money;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn payment_status_strategy_accepts_existing_wire_values() {
    let status = PaymentStatusStrategy::parse("PAID").expect("known status");

    assert_eq!("PAID", status.as_wire_value());
}

#[test]
fn wallet_validation_chain_rejects_zero_amount() {
    let result = WalletValidationChain::wallet_mutation().validate(&WalletCommandContext {
        user_id: "user-1",
        role: "BUYER",
        amount: Some(Money::zero()),
        correlation_id: None,
        status: None,
    });

    assert!(result.is_err());
}

struct FakePaymentGateway {
    create_payment_calls: AtomicUsize,
}

#[tonic::async_trait]
impl PaymentGateway for FakePaymentGateway {
    async fn create_payment(
        &self,
        payment_id: &str,
        _amount: Money,
        _payment_method: Option<&str>,
    ) -> Result<PaymentPage, GatewayError> {
        self.create_payment_calls.fetch_add(1, Ordering::SeqCst);
        Ok(PaymentPage {
            redirect_url: format!("https://payments.test/{payment_id}"),
            va_number: Some("123456".to_string()),
            payment_channel: Some("fake-va".to_string()),
        })
    }

    async fn validate_bank_account(
        &self,
        bank_code: &str,
        account_number: &str,
    ) -> Result<ValidatedBankAccount, GatewayError> {
        Ok(ValidatedBankAccount {
            bank_code: bank_code.to_string(),
            account_number: account_number.to_string(),
            account_name: "Fake Account".to_string(),
        })
    }

    async fn create_payout(
        &self,
        _user_id: &str,
        _amount: Money,
        _account: &ValidatedBankAccount,
    ) -> Result<PayoutResult, GatewayError> {
        Ok(PayoutResult {
            reference_no: "fake-payout".to_string(),
        })
    }

    async fn fetch_transaction_status(&self, _payment_id: &str) -> Result<String, GatewayError> {
        Ok("settlement".to_string())
    }
}

#[tokio::test]
async fn wallet_service_uses_injected_payment_gateway() {
    let pool = connect_pool("sqlite::memory:").await.expect("connect db");
    run_migrations(&pool).await.expect("migrate db");
    let gateway = Arc::new(FakePaymentGateway {
        create_payment_calls: AtomicUsize::new(0),
    });
    let service = WalletService::new_with_gateway(pool, gateway.clone());

    service
        .create_wallet("buyer-1", "BUYER")
        .await
        .expect("create wallet");
    let payment = service
        .create_top_up_intent(
            "buyer-1",
            "BUYER",
            Money::from_cents(25_000),
            Some("bca_va"),
            Some("intent-1"),
        )
        .await
        .expect("create payment intent");

    assert!(payment.redirect_url.starts_with("https://payments.test/"));
    assert_eq!(gateway.create_payment_calls.load(Ordering::SeqCst), 1);
}
