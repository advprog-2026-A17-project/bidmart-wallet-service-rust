use bidmart_wallet_service_rust::wallet::{
    Money, TransactionType, Wallet, WalletError, WalletTransaction,
};

// ── Money newtype tests ──────────────────────────────────────────

#[test]
fn money_from_cents_and_back() {
    let m = Money::from_cents(12345);
    assert_eq!(m.cents(), 12345);
}

#[test]
fn money_zero() {
    let m = Money::zero();
    assert_eq!(m.cents(), 0);
}

#[test]
fn money_add() {
    let a = Money::from_cents(100);
    let b = Money::from_cents(250);
    assert_eq!((a + b).cents(), 350);
}

#[test]
fn money_display_formats_correctly() {
    let m = Money::from_cents(10050);
    let display = format!("{m}");
    assert_eq!(display, "100.50");
}

// ── Wallet creation tests ────────────────────────────────────────

#[test]
fn new_wallet_has_zero_balances() {
    let w = Wallet::new("user-1");
    assert_eq!(w.user_id(), "user-1");
    assert_eq!(w.active_balance(), Money::zero());
    assert_eq!(w.held_balance(), Money::zero());
    assert!(!w.id().is_empty());
}

// ── Top-up tests ─────────────────────────────────────────────────

#[test]
fn top_up_increases_active_balance() {
    let mut w = Wallet::new("user-1");
    let tx = w.top_up(Money::from_cents(5000)).unwrap();
    assert_eq!(w.active_balance(), Money::from_cents(5000));
    assert_eq!(tx.transaction_type, TransactionType::TopUp);
    assert_eq!(tx.amount, Money::from_cents(5000));
}

#[test]
fn top_up_zero_amount_fails() {
    let mut w = Wallet::new("user-1");
    let result = w.top_up(Money::zero());
    assert_eq!(result, Err(WalletError::InvalidAmount));
}

// ── Withdraw tests ───────────────────────────────────────────────

#[test]
fn withdraw_decreases_active_balance() {
    let mut w = Wallet::new("user-1");
    w.top_up(Money::from_cents(10000)).unwrap();
    let tx = w.withdraw(Money::from_cents(3000)).unwrap();
    assert_eq!(w.active_balance(), Money::from_cents(7000));
    assert_eq!(tx.transaction_type, TransactionType::Withdraw);
}

#[test]
fn withdraw_more_than_active_balance_fails() {
    let mut w = Wallet::new("user-1");
    w.top_up(Money::from_cents(1000)).unwrap();
    let result = w.withdraw(Money::from_cents(2000));
    assert_eq!(result, Err(WalletError::InsufficientActiveBalance));
}

#[test]
fn withdraw_zero_fails() {
    let mut w = Wallet::new("user-1");
    let result = w.withdraw(Money::zero());
    assert_eq!(result, Err(WalletError::InvalidAmount));
}

// ── Hold funds tests ─────────────────────────────────────────────

#[test]
fn hold_moves_from_active_to_held() {
    let mut w = Wallet::new("user-1");
    w.top_up(Money::from_cents(10000)).unwrap();
    let tx = w.hold(Money::from_cents(3000)).unwrap();
    assert_eq!(w.active_balance(), Money::from_cents(7000));
    assert_eq!(w.held_balance(), Money::from_cents(3000));
    assert_eq!(tx.transaction_type, TransactionType::Hold);
}

#[test]
fn hold_more_than_active_balance_fails() {
    let mut w = Wallet::new("user-1");
    w.top_up(Money::from_cents(1000)).unwrap();
    let result = w.hold(Money::from_cents(2000));
    assert_eq!(result, Err(WalletError::InsufficientActiveBalance));
}

#[test]
fn hold_zero_fails() {
    let mut w = Wallet::new("user-1");
    let result = w.hold(Money::zero());
    assert_eq!(result, Err(WalletError::InvalidAmount));
}

// ── Release funds tests ──────────────────────────────────────────

#[test]
fn release_moves_from_held_to_active() {
    let mut w = Wallet::new("user-1");
    w.top_up(Money::from_cents(10000)).unwrap();
    w.hold(Money::from_cents(5000)).unwrap();
    let tx = w.release(Money::from_cents(3000)).unwrap();
    assert_eq!(w.active_balance(), Money::from_cents(8000));
    assert_eq!(w.held_balance(), Money::from_cents(2000));
    assert_eq!(tx.transaction_type, TransactionType::Release);
}

#[test]
fn release_more_than_held_balance_fails() {
    let mut w = Wallet::new("user-1");
    w.top_up(Money::from_cents(5000)).unwrap();
    w.hold(Money::from_cents(2000)).unwrap();
    let result = w.release(Money::from_cents(3000));
    assert_eq!(result, Err(WalletError::InsufficientHeldBalance));
}

#[test]
fn release_zero_fails() {
    let mut w = Wallet::new("user-1");
    let result = w.release(Money::zero());
    assert_eq!(result, Err(WalletError::InvalidAmount));
}

// ── Convert held funds tests ─────────────────────────────────────

#[test]
fn convert_removes_from_held_balance() {
    let mut w = Wallet::new("user-1");
    w.top_up(Money::from_cents(10000)).unwrap();
    w.hold(Money::from_cents(5000)).unwrap();
    let tx = w.convert(Money::from_cents(5000)).unwrap();
    assert_eq!(w.held_balance(), Money::zero());
    assert_eq!(w.active_balance(), Money::from_cents(5000));
    assert_eq!(tx.transaction_type, TransactionType::Convert);
}

#[test]
fn convert_more_than_held_balance_fails() {
    let mut w = Wallet::new("user-1");
    w.top_up(Money::from_cents(5000)).unwrap();
    w.hold(Money::from_cents(2000)).unwrap();
    let result = w.convert(Money::from_cents(3000));
    assert_eq!(result, Err(WalletError::InsufficientHeldBalance));
}

#[test]
fn convert_zero_fails() {
    let mut w = Wallet::new("user-1");
    let result = w.convert(Money::zero());
    assert_eq!(result, Err(WalletError::InvalidAmount));
}

// ── Bid (hold variant) tests ─────────────────────────────────────

#[test]
fn bid_moves_from_active_to_held() {
    let mut w = Wallet::new("user-1");
    w.top_up(Money::from_cents(10000)).unwrap();
    let tx = w.bid(Money::from_cents(4000)).unwrap();
    assert_eq!(w.active_balance(), Money::from_cents(6000));
    assert_eq!(w.held_balance(), Money::from_cents(4000));
    assert_eq!(tx.transaction_type, TransactionType::Bid);
}

#[test]
fn bid_insufficient_balance_fails() {
    let mut w = Wallet::new("user-1");
    let result = w.bid(Money::from_cents(1000));
    assert_eq!(result, Err(WalletError::InsufficientActiveBalance));
}

// ── Transaction type display ─────────────────────────────────────

#[test]
fn transaction_type_display() {
    assert_eq!(TransactionType::TopUp.as_str(), "TOP_UP");
    assert_eq!(TransactionType::Withdraw.as_str(), "WITHDRAW");
    assert_eq!(TransactionType::Hold.as_str(), "HOLD");
    assert_eq!(TransactionType::Release.as_str(), "RELEASE");
    assert_eq!(TransactionType::Convert.as_str(), "CONVERT");
    assert_eq!(TransactionType::Bid.as_str(), "BID");
    assert_eq!(TransactionType::CancelBid.as_str(), "CANCEL_BID");
    assert_eq!(TransactionType::TopUpFailed.as_str(), "TOP_UP_FAILED");
    assert_eq!(TransactionType::TopUpExpired.as_str(), "TOP_UP_EXPIRED");
    assert_eq!(TransactionType::WithdrawFailed.as_str(), "WITHDRAW_FAILED");
    assert_eq!(TransactionType::WithdrawExpired.as_str(), "WITHDRAW_EXPIRED");
}

// ── WalletTransaction creation ───────────────────────────────────

#[test]
fn wallet_transaction_captures_user_and_amount() {
    let tx = WalletTransaction::new("user-1", TransactionType::TopUp, Money::from_cents(500));
    assert_eq!(tx.user_id, "user-1");
    assert_eq!(tx.transaction_type, TransactionType::TopUp);
    assert_eq!(tx.amount, Money::from_cents(500));
    assert!(!tx.id.is_empty());
}
