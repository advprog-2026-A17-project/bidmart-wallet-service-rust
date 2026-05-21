use std::sync::Arc;
use std::time::Instant;

use bidmart_wallet_service_rust::server;
use bidmart_wallet_service_rust::service::wallet_service::WalletService;
use bidmart_wallet_service_rust::wallet::Money;

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.state
    }
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    let index = ((sorted.len() * percentile) / 100).min(sorted.len() - 1);
    sorted[index]
}

#[tokio::test]
#[ignore = "manual load harness"]
async fn run_seeded_wallet_hold_load_harness() {
    let pool = server::connect_pool("sqlite::memory:")
        .await
        .expect("connect to in-memory db");
    server::run_migrations(&pool).await.expect("run migrations");

    let service = Arc::new(WalletService::new(pool.clone()));
    let wallet_count = 80usize;
    let attempts = 400usize;
    let hold_amount_cents = 1_000u64;
    let initial_balance_cents = hold_amount_cents * attempts as u64;

    for idx in 0..wallet_count {
        let user_id = format!("load-wallet-user-{idx}");
        service
            .create_wallet(&user_id, "BUYER")
            .await
            .expect("create wallet");
        service
            .top_up(&user_id, "BUYER", Money::from_cents(initial_balance_cents))
            .await
            .expect("seed wallet balance");
    }

    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
    let mut rng = Lcg::new(20260521);
    let mut handles = Vec::with_capacity(attempts);

    for idx in 0..attempts {
        let service = service.clone();
        let random = rng.next();
        let user_id = format!("load-wallet-user-{}", random as usize % wallet_count);
        let auction_id = format!("load-auction-{}", random % 20);
        let bid_id = format!("load-bid-{idx}");
        let hold_id = format!("load-hold-{idx}");
        let expires_at = expires_at.clone();

        handles.push(tokio::spawn(async move {
            let started = Instant::now();
            let result = service
                .hold_funds(
                    &user_id,
                    "BUYER",
                    &auction_id,
                    &bid_id,
                    Money::from_cents(hold_amount_cents),
                    &hold_id,
                    &expires_at,
                )
                .await;
            (result.is_ok(), started.elapsed().as_millis() as u64)
        }));
    }

    let mut accepted = 0usize;
    let mut latencies_ms = Vec::with_capacity(attempts);
    for handle in handles {
        let (ok, latency_ms) = handle.await.expect("join load task");
        if ok {
            accepted += 1;
        }
        latencies_ms.push(latency_ms);
    }

    latencies_ms.sort_unstable();
    let p50 = percentile(&latencies_ms, 50);
    let p95 = percentile(&latencies_ms, 95);
    let p99 = percentile(&latencies_ms, 99);
    let max = *latencies_ms.last().expect("max latency");
    let apdex_t_ms = 100u64;
    let satisfied = latencies_ms
        .iter()
        .filter(|latency| **latency <= apdex_t_ms)
        .count();
    let tolerating = latencies_ms
        .iter()
        .filter(|latency| **latency > apdex_t_ms && **latency <= apdex_t_ms * 4)
        .count();
    let apdex_score = (satisfied as f64 + (tolerating as f64 / 2.0)) / latencies_ms.len() as f64;

    let persisted_holds: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM holds")
        .fetch_one(&pool)
        .await
        .expect("count holds");
    let hold_transactions: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM wallet_transactions WHERE transaction_type = 'HOLD'")
            .fetch_one(&pool)
            .await
            .expect("count hold transactions");

    assert_eq!(accepted, attempts);
    assert_eq!(persisted_holds.0 as usize, accepted);
    assert_eq!(hold_transactions.0 as usize, accepted);

    println!(
        "wallet-load-harness seed=20260521 attempts={attempts} wallets={wallet_count} accepted={accepted} holds={} hold_transactions={} p50_ms={p50} p95_ms={p95} p99_ms={p99} max_ms={max} apdex_t_ms={apdex_t_ms} apdex={apdex_score:.3} satisfied={satisfied} tolerating={tolerating}",
        persisted_holds.0, hold_transactions.0
    );
}
