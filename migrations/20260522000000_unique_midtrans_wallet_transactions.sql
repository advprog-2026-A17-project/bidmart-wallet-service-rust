CREATE UNIQUE INDEX IF NOT EXISTS uq_wallet_transactions_midtrans_correlation
    ON wallet_transactions(source_service, correlation_id, transaction_type)
    WHERE source_service IS NOT NULL
      AND correlation_id IS NOT NULL
      AND transaction_type = 'TOP_UP';
