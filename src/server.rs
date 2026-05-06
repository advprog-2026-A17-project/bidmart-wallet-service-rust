use axum::Router;
use sqlx::AnyPool;
use sqlx::any::{AnyPoolOptions, install_default_drivers};

use crate::http::router::create_router;
use crate::service::wallet_service::WalletService;

pub fn default_database_url() -> String {
    "postgresql://postgres:postgres@localhost:5432/bidmart_wallet_db".to_string()
}

pub fn build_router(pool: AnyPool) -> Router {
    let service = WalletService::new(pool);
    create_router(service)
}

pub async fn connect_pool(database_url: &str) -> Result<AnyPool, sqlx::Error> {
    install_default_drivers();
    let max_connections = if database_url == "sqlite::memory:" {
        1
    } else {
        5
    };

    AnyPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await
}

pub async fn run_migrations(pool: &AnyPool) -> Result<(), sqlx::Error> {
    let sql = include_str!("../migrations/20260429000000_init.sql");
    for statement in sql.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() {
            sqlx::query(trimmed).execute(pool).await?;
        }
    }
    Ok(())
}
