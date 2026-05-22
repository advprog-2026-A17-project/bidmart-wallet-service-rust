use crate::service::wallet_service::{ServiceError, WalletService};
use crate::wallet::{Hold, Money, PaymentIntent, Wallet, WalletWithdrawal};

pub struct CreateWalletHandler<'a> {
    service: &'a WalletService,
}

impl<'a> CreateWalletHandler<'a> {
    pub fn new(service: &'a WalletService) -> Self {
        Self { service }
    }

    pub async fn handle(&self, user_id: &str, role: &str) -> Result<Wallet, ServiceError> {
        self.service.create_wallet_core(user_id, role).await
    }
}

pub struct WalletMutationHandler<'a> {
    service: &'a WalletService,
}

impl<'a> WalletMutationHandler<'a> {
    pub fn new(service: &'a WalletService) -> Self {
        Self { service }
    }

    pub async fn top_up(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.service.top_up_core(user_id, role, amount).await
    }

    pub async fn withdraw(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.service.withdraw_core(user_id, role, amount).await
    }

    pub async fn hold(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.service.hold_core(user_id, role, amount).await
    }

    pub async fn release(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.service.release_core(user_id, role, amount).await
    }

    pub async fn convert(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
    ) -> Result<Wallet, ServiceError> {
        self.service.convert_core(user_id, role, amount).await
    }
}

pub struct TopUpIntentHandler<'a> {
    service: &'a WalletService,
}

impl<'a> TopUpIntentHandler<'a> {
    pub fn new(service: &'a WalletService) -> Self {
        Self { service }
    }

    pub async fn handle(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
        payment_method: Option<&str>,
        idempotency_key: Option<&str>,
    ) -> Result<PaymentIntent, ServiceError> {
        self.service
            .create_top_up_intent_core(user_id, role, amount, payment_method, idempotency_key)
            .await
    }
}

pub struct HoldFundsHandler<'a> {
    service: &'a WalletService,
}

impl<'a> HoldFundsHandler<'a> {
    pub fn new(service: &'a WalletService) -> Self {
        Self { service }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn handle(
        &self,
        user_id: &str,
        role: &str,
        auction_id: &str,
        bid_id: &str,
        amount: Money,
        hold_id: &str,
        expires_at: &str,
    ) -> Result<Hold, ServiceError> {
        self.service
            .hold_funds_core(
                user_id, role, auction_id, bid_id, amount, hold_id, expires_at,
            )
            .await
    }
}

pub struct PayoutHandler<'a> {
    service: &'a WalletService,
}

impl<'a> PayoutHandler<'a> {
    pub fn new(service: &'a WalletService) -> Self {
        Self { service }
    }

    pub async fn create_withdrawal(
        &self,
        user_id: &str,
        role: &str,
        amount: Money,
        bank_code: &str,
        account_number: &str,
        idempotency_key: Option<&str>,
    ) -> Result<WalletWithdrawal, ServiceError> {
        self.service
            .create_withdrawal_core(
                user_id,
                role,
                amount,
                bank_code,
                account_number,
                idempotency_key,
            )
            .await
    }
}

pub struct ProvisionWalletHandler<'a> {
    service: &'a WalletService,
}

impl<'a> ProvisionWalletHandler<'a> {
    pub fn new(service: &'a WalletService) -> Self {
        Self { service }
    }

    pub async fn handle(
        &self,
        event_id: &str,
        user_id: &str,
        email: &str,
        role: &str,
        source: &str,
    ) -> Result<(), ServiceError> {
        self.service
            .provision_wallet_core(event_id, user_id, email, role, source)
            .await
    }
}
