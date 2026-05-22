CREATE TABLE IF NOT EXISTS wallets (
    id                   TEXT PRIMARY KEY,
    user_id              TEXT NOT NULL,
    role                 TEXT NOT NULL DEFAULT 'BUYER',
    active_balance INTEGER NOT NULL DEFAULT 0 CHECK (active_balance >= 0),
    held_balance   INTEGER NOT NULL DEFAULT 0 CHECK (held_balance >= 0),
    version              INTEGER NOT NULL DEFAULT 0,
    UNIQUE(user_id, role)
);

CREATE TABLE IF NOT EXISTS wallet_transactions (
    id               TEXT PRIMARY KEY,
    user_id          TEXT NOT NULL,
    transaction_type TEXT NOT NULL,
    amount     INTEGER NOT NULL CHECK (amount >= 0),
    created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    correlation_id   TEXT,
    source_service   TEXT
);

CREATE INDEX IF NOT EXISTS idx_wallet_transactions_user_id
    ON wallet_transactions(user_id);

CREATE TABLE IF NOT EXISTS wallet_provisioning_events (
    event_id     TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL,
    email        TEXT NOT NULL,
    occurred_at  TEXT NOT NULL,
    source       TEXT NOT NULL,
    processed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS holds (
    id         TEXT PRIMARY KEY,
    wallet_id  TEXT NOT NULL,
    auction_id TEXT NOT NULL,
    bid_id     TEXT NOT NULL,
    amount     INTEGER NOT NULL CHECK (amount >= 0),
    status     TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_holds_auction_bid ON holds(auction_id, bid_id);
CREATE INDEX IF NOT EXISTS idx_holds_wallet ON holds(wallet_id);

CREATE TABLE IF NOT EXISTS wallet_payment_intents (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL,
    role         TEXT NOT NULL DEFAULT 'BUYER',
    amount INTEGER NOT NULL CHECK (amount >= 0),
    status       TEXT NOT NULL,
    redirect_url TEXT NOT NULL,
    va_number    TEXT,
    payment_channel TEXT,
    idempotency_key TEXT,
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_wallet_payment_intents_user_id
    ON wallet_payment_intents(user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_wallet_payment_intents_idempotency
    ON wallet_payment_intents(user_id, role, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS wallet_withdrawals (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL,
    role         TEXT NOT NULL DEFAULT 'BUYER',
    amount INTEGER NOT NULL CHECK (amount >= 0),
    bank_account TEXT NOT NULL,
    bank_code    TEXT,
    account_number TEXT,
    account_name TEXT,
    payout_reference TEXT,
    failure_reason TEXT,
    idempotency_key TEXT,
    status       TEXT NOT NULL,
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_wallet_withdrawals_user_id
    ON wallet_withdrawals(user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_wallet_withdrawals_idempotency
    ON wallet_withdrawals(user_id, role, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
