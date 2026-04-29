CREATE TABLE IF NOT EXISTS wallets (
    id          TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL UNIQUE,
    active_balance_cents INTEGER NOT NULL DEFAULT 0,
    held_balance_cents   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS wallet_transactions (
    id               TEXT PRIMARY KEY,
    user_id          TEXT NOT NULL,
    transaction_type TEXT NOT NULL,
    amount_cents     INTEGER NOT NULL,
    created_at       TEXT NOT NULL DEFAULT (datetime('now'))
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
