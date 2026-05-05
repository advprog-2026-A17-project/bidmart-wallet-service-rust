use std::time::Duration;
use crate::persistence::repositories::WalletRepository;
use std::sync::Arc;

pub async fn run_reconciliation_worker(repo: Arc<WalletRepository>) {
    println!("Reconciliation worker started...");
    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        interval.tick().await;
        match repo.find_expired_holds().await {
            Ok(expired_ids) => {
                for hold_id in expired_ids {
                    println!("🧹 Reconciling expired hold: {}", hold_id);
                    if let Err(e) = repo.release_funds(&hold_id).await {
                        eprintln!("Failed to auto-release hold {}: {}", hold_id, e);
                    } else {
                        println!("Auto-released hold {}", hold_id);
                    }
                }
            }
            Err(e) => eprintln!("Worker error fetching expired holds: {}", e),
        }
    }
}