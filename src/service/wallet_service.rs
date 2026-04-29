use sqlx::SqlitePool;

use crate::persistence::repositories::{TransactionRepository, WalletRepository};
use crate::wallet::{Money, Wallet, WalletError, WalletTransaction};

/// Service-level error combining domain and persistence failures.
#[derive(Debug)]
pub enum ServiceError {
    WalletNotFound(String),
    Domain(WalletError),
    Persistence(sqlx::Error),
    TransactionNotFound(String),
    ForbiddenAccess,
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
        }
    }
}

/// Orchestrates wallet use cases by coordinating domain logic and persistence.
pub struct WalletService {
    wallet_repo: WalletRepository,
    tx_repo: TransactionRepository,
}

impl WalletService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            wallet_repo: WalletRepository::new(pool.clone()),
            tx_repo: TransactionRepository::new(pool),
        }
    }

    pub async fn create_wallet(&self, user_id: &str) -> Result<Wallet, ServiceError> {
        let wallet = Wallet::new(user_id);
        self.wallet_repo.insert(&wallet).await?;
        Ok(wallet)
    }

    pub async fn find_by_user_id(&self, user_id: &str) -> Result<Wallet, ServiceError> {
        self.wallet_repo
            .find_by_user_id(user_id)
            .await?
            .ok_or_else(|| ServiceError::WalletNotFound(user_id.to_string()))
    }

    pub async fn find_all(&self) -> Result<Vec<Wallet>, ServiceError> {
        Ok(self.wallet_repo.find_all().await?)
    }

    pub async fn top_up(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        let mut wallet = self.find_by_user_id(user_id).await?;
        let tx = wallet.top_up(amount)?;
        self.tx_repo.insert(&tx).await?;
        self.wallet_repo.update(&wallet).await?;
        Ok(wallet)
    }

    pub async fn withdraw(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        let mut wallet = self.find_by_user_id(user_id).await?;
        let tx = wallet.withdraw(amount)?;
        self.tx_repo.insert(&tx).await?;
        self.wallet_repo.update(&wallet).await?;
        Ok(wallet)
    }

    pub async fn hold(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        let mut wallet = self.find_by_user_id(user_id).await?;
        let tx = wallet.hold(amount)?;
        self.tx_repo.insert(&tx).await?;
        self.wallet_repo.update(&wallet).await?;
        Ok(wallet)
    }

    pub async fn release(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        let mut wallet = self.find_by_user_id(user_id).await?;
        let tx = wallet.release(amount)?;
        self.tx_repo.insert(&tx).await?;
        self.wallet_repo.update(&wallet).await?;
        Ok(wallet)
    }

    pub async fn convert(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        let mut wallet = self.find_by_user_id(user_id).await?;
        let tx = wallet.convert(amount)?;
        self.tx_repo.insert(&tx).await?;
        self.wallet_repo.update(&wallet).await?;
        Ok(wallet)
    }

    pub async fn bid(&self, user_id: &str, amount: Money) -> Result<Wallet, ServiceError> {
        let mut wallet = self.find_by_user_id(user_id).await?;
        let tx = wallet.bid(amount)?;
        self.tx_repo.insert(&tx).await?;
        self.wallet_repo.update(&wallet).await?;
        Ok(wallet)
    }

    pub async fn cancel_bid(&self, user_id: &str, bid_tx_id: &str) -> Result<(), ServiceError> {
        let mut wallet = self.find_by_user_id(user_id).await?;
        let tx = self
            .tx_repo
            .find_by_id(bid_tx_id)
            .await?
            .ok_or_else(|| ServiceError::TransactionNotFound(bid_tx_id.to_string()))?;

        if tx.user_id != user_id {
            return Err(ServiceError::ForbiddenAccess);
        }

        let release_tx = wallet.release(tx.amount)?;
        self.tx_repo.insert(&release_tx).await?;
        self.wallet_repo.update(&wallet).await?;
        Ok(())
    }

    pub async fn get_transaction_history(
        &self,
        user_id: &str,
    ) -> Result<Vec<WalletTransaction>, ServiceError> {
        Ok(self.tx_repo.find_by_user_id(user_id).await?)
    }
}
