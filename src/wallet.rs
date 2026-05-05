use std::fmt;
use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Money ────────────────────────────────────────────────────────

/// Monetary value stored as whole cents to avoid floating-point precision issues.
///
/// This is a value object — immutable, comparable, and safe for arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

impl Sub for Money {
    type Output = Self;

    /// Panics if subtraction would underflow. Callers must validate first.
    fn sub(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_sub(rhs.0)
                .expect("Money subtraction underflow"),
        )
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:02}", self.0 / 100, self.0 % 100)
    }
}

// ── TransactionType ──────────────────────────────────────────────

/// Transaction types matching the Java service wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
            Self::TopUp => "TOP_UP",
            Self::Withdraw => "WITHDRAW",
            Self::Hold => "HOLD",
            Self::Release => "RELEASE",
            Self::Convert => "CONVERT",
            Self::Bid => "BID",
            Self::CancelBid => "CANCEL_BID",
        }
    }

    /// Parse a wire-format string back into a `TransactionType`.
    ///
    /// Panics on unknown values — only valid DB data should reach here.
    pub fn from_str(s: &str) -> Self {
        match s {
            "TOP_UP" => Self::TopUp,
            "WITHDRAW" => Self::Withdraw,
            "HOLD" => Self::Hold,
            "RELEASE" => Self::Release,
            "CONVERT" => Self::Convert,
            "BID" => Self::Bid,
            "CANCEL_BID" => Self::CancelBid,
            other => panic!("unknown transaction type: {other}"),
        }
    }
}

impl fmt::Display for TransactionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── WalletError ──────────────────────────────────────────────────

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum WalletError {
    #[error("amount must be greater than zero")]
    InvalidAmount,
    #[error("insufficient active balance")]
    InsufficientActiveBalance,
    #[error("insufficient held balance")]
    InsufficientHeldBalance,
}

// ── WalletTransaction ────────────────────────────────────────────

/// Immutable record of a single wallet mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletTransaction {
    pub id: String,
    pub user_id: String,
    pub transaction_type: TransactionType,
    pub amount: Money,
    pub correlation_id: Option<String>,
    pub source_service: Option<String>,
}

impl WalletTransaction {
    pub fn new(user_id: &str, transaction_type: TransactionType, amount: Money) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            transaction_type,
            amount,
            correlation_id: None,
            source_service: None,
        }
    }
}

// ── Wallet ───────────────────────────────────────────────────────

/// Core wallet domain entity.
///
/// Encapsulates active and held balances with domain-level validation.
/// Every mutating method returns a `WalletTransaction` receipt on success,
/// enabling the service layer to persist the audit trail independently.
#[derive(Debug, Clone)]
pub struct Wallet {
    id: String,
    user_id: String,
    active_balance: Money,
    held_balance: Money,
}

impl Wallet {
    /// Create a brand-new wallet with zero balances.
    pub fn new(user_id: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            active_balance: Money::zero(),
            held_balance: Money::zero(),
        }
    }

    /// Reconstruct a wallet from persisted data.
    pub fn with_balances(
        id: String,
        user_id: String,
        active_balance: Money,
        held_balance: Money,
    ) -> Self {
        Self {
            id,
            user_id,
            active_balance,
            held_balance,
        }
    }

    // ── Accessors ────────────────────────────────────────────────

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

    // ── Balance operations ───────────────────────────────────────

    pub fn top_up(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        Self::validate_positive(amount)?;
        self.active_balance = self.active_balance + amount;
        Ok(self.record(TransactionType::TopUp, amount))
    }

    pub fn withdraw(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        Self::validate_positive(amount)?;
        self.require_active_balance(amount)?;
        self.active_balance = self.active_balance - amount;
        Ok(self.record(TransactionType::Withdraw, amount))
    }

    pub fn hold(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        Self::validate_positive(amount)?;
        self.require_active_balance(amount)?;
        self.active_balance = self.active_balance - amount;
        self.held_balance = self.held_balance + amount;
        Ok(self.record(TransactionType::Hold, amount))
    }

    pub fn release(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        Self::validate_positive(amount)?;
        self.require_held_balance(amount)?;
        self.held_balance = self.held_balance - amount;
        self.active_balance = self.active_balance + amount;
        Ok(self.record(TransactionType::Release, amount))
    }

    pub fn convert(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        Self::validate_positive(amount)?;
        self.require_held_balance(amount)?;
        self.held_balance = self.held_balance - amount;
        Ok(self.record(TransactionType::Convert, amount))
    }

    pub fn bid(&mut self, amount: Money) -> Result<WalletTransaction, WalletError> {
        Self::validate_positive(amount)?;
        self.require_active_balance(amount)?;
        self.active_balance = self.active_balance - amount;
        self.held_balance = self.held_balance + amount;
        Ok(self.record(TransactionType::Bid, amount))
    }

    // ── Private helpers (DRY) ────────────────────────────────────

    fn validate_positive(amount: Money) -> Result<(), WalletError> {
        if amount.is_zero() {
            return Err(WalletError::InvalidAmount);
        }
        Ok(())
    }

    fn require_active_balance(&self, amount: Money) -> Result<(), WalletError> {
        if self.active_balance < amount {
            return Err(WalletError::InsufficientActiveBalance);
        }
        Ok(())
    }

    fn require_held_balance(&self, amount: Money) -> Result<(), WalletError> {
        if self.held_balance < amount {
            return Err(WalletError::InsufficientHeldBalance);
        }
        Ok(())
    }

    fn record(&self, tx_type: TransactionType, amount: Money) -> WalletTransaction {
        WalletTransaction::new(&self.user_id, tx_type, amount)
    }
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HoldStatus {
    Active,
    Released,
    Converted,
}

impl std::fmt::Display for HoldStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status_str = match self {
            HoldStatus::Active => "ACTIVE",
            HoldStatus::Released => "RELEASED",
            HoldStatus::Converted => "CONVERTED",
        };
        write!(f, "{}", status_str)
    }
}

impl std::str::FromStr for HoldStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACTIVE" => Ok(HoldStatus::Active),
            "RELEASED" => Ok(HoldStatus::Released),
            "CONVERTED" => Ok(HoldStatus::Converted),
            _ => Err(format!("Unknown hold status: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hold {
    pub id: String,
    pub wallet_id: String,
    pub auction_id: String,
    pub bid_id: String,
    pub amount: i64,
    pub status: HoldStatus,
    pub expires_at: String,
    pub created_at: String,
    pub updated_at: String,
}
