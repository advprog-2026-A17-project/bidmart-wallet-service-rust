CREATE TABLE IF NOT EXISTS wallets (
    id                   TEXT PRIMARY KEY,
    user_id              TEXT NOT NULL UNIQUE,
    active_balance_cents INTEGER NOT NULL DEFAULT 0,
    held_balance_cents   INTEGER NOT NULL DEFAULT 0,
    version              INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS wallet_transactions (
    id               TEXT PRIMARY KEY,
    user_id          TEXT NOT NULL,
    transaction_type TEXT NOT NULL,
    amount_cents     INTEGER NOT NULL,
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
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
    processed_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS holds (
    id         TEXT PRIMARY KEY,
    wallet_id  TEXT NOT NULL,
    auction_id TEXT NOT NULL,
    bid_id     TEXT NOT NULL,
    amount     INTEGER NOT NULL,
    status     TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_holds_auction_bid ON holds(auction_id, bid_id);
CREATE INDEX IF NOT EXISTS idx_holds_wallet ON holds(wallet_id);

CREATE TABLE IF NOT EXISTS wallet_payment_intents (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL,
    amount_cents INTEGER NOT NULL,
    status       TEXT NOT NULL,
    redirect_url TEXT NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_wallet_payment_intents_user_id
    ON wallet_payment_intents(user_id);

CREATE TABLE IF NOT EXISTS wallet_withdrawals (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL,
    amount_cents INTEGER NOT NULL,
    bank_account TEXT NOT NULL,
    status       TEXT NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_wallet_withdrawals_user_id
    ON wallet_withdrawals(user_id);
