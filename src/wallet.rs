use std::fmt;
use std::ops::Add;

use thiserror::Error;

/// Monetary value stored as whole cents to avoid floating-point issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Money(u64);

impl Money {
    pub fn from_cents(value: u64) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn cents(self) -> u64 {
        self.0
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl Add for Money {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:02}", self.0 / 100, self.0 % 100)
    }
}

/// Transaction types matching the Java service contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionType {
    TopUp,
    Withdraw,
    Hold,
    Release,
    Convert,
    Bid,
    CancelBid,
}

impl TransactionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransactionType::TopUp => "TOP_UP",
            TransactionType::Withdraw => "WITHDRAW",
            TransactionType::Hold => "HOLD",
            TransactionType::Release => "RELEASE",
            TransactionType::Convert => "CONVERT",
            TransactionType::Bid => "BID",
            TransactionType::CancelBid => "CANCEL_BID",
        }
    }
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum WalletError {
    #[error("amount must be greater than zero")]
    InvalidAmount,
    #[error("insufficient active balance")]
    InsufficientActiveBalance,
    #[error("insufficient held balance")]
    InsufficientHeldBalance,
}

/// A record of a single wallet mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletTransaction {
    pub id: String,
    pub user_id: String,
    pub transaction_type: TransactionType,
    pub amount: Money,
}

impl WalletTransaction {
    pub fn new(user_id: &str, transaction_type: TransactionType, amount: Money) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            transaction_type,
            amount,
        }
    }
}

/// Core wallet domain entity with active and held balances.
#[derive(Debug, Clone)]
pub struct Wallet {
    id: String,
    user_id: String,
    active_balance: Money,
    held_balance: Money,
}

impl Wallet {
    pub fn new(user_id: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            active_balance: Money::zero(),
            held_balance: Money::zero(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn active_balance(&self) -> Money {
        self.active_balance
    }

    pub fn held_balance(&self) -> Money {
        self.held_balance
    }

    pub fn top_up(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        if amount.is_zero() {
            return Err(WalletError::InvalidAmount);
        }
        self.active_balance = self.active_balance + amount;
        Ok(WalletTransaction::new(&self.user_id, TransactionType::TopUp, amount))
    }

    pub fn withdraw(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        if amount.is_zero() {
            return Err(WalletError::InvalidAmount);
        }
        if self.active_balance < amount {
            return Err(WalletError::InsufficientActiveBalance);
        }
        self.active_balance = Money::from_cents(self.active_balance.cents() - amount.cents());
        Ok(WalletTransaction::new(&self.user_id, TransactionType::Withdraw, amount))
    }

    pub fn hold(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        if amount.is_zero() {
            return Err(WalletError::InvalidAmount);
        }
        if self.active_balance < amount {
            return Err(WalletError::InsufficientActiveBalance);
        }
        self.active_balance = Money::from_cents(self.active_balance.cents() - amount.cents());
        self.held_balance = self.held_balance + amount;
        Ok(WalletTransaction::new(&self.user_id, TransactionType::Hold, amount))
    }

    pub fn release(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        if amount.is_zero() {
            return Err(WalletError::InvalidAmount);
        }
        if self.held_balance < amount {
            return Err(WalletError::InsufficientHeldBalance);
        }
        self.held_balance = Money::from_cents(self.held_balance.cents() - amount.cents());
        self.active_balance = self.active_balance + amount;
        Ok(WalletTransaction::new(&self.user_id, TransactionType::Release, amount))
    }

    pub fn convert(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        if amount.is_zero() {
            return Err(WalletError::InvalidAmount);
        }
        if self.held_balance < amount {
            return Err(WalletError::InsufficientHeldBalance);
        }
        self.held_balance = Money::from_cents(self.held_balance.cents() - amount.cents());
        Ok(WalletTransaction::new(&self.user_id, TransactionType::Convert, amount))
    }

    pub fn bid(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        if amount.is_zero() {
            return Err(WalletError::InvalidAmount);
        }
        if self.active_balance < amount {
            return Err(WalletError::InsufficientActiveBalance);
        }
        self.active_balance = Money::from_cents(self.active_balance.cents() - amount.cents());
        self.held_balance = self.held_balance + amount;
        Ok(WalletTransaction::new(&self.user_id, TransactionType::Bid, amount))
    }
}
