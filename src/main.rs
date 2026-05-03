use std::env;

use bidmart_wallet_service_rust::server;
use dotenvy::from_path;

#[tokio::main]
async fn main() {
    let _ = from_path(".env");
    let _ = dotenvy::from_path_override("../bidmart-infrastructure/.env");

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:wallet.db".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "8083".to_string());
    let addr = format!("0.0.0.0:{port}");

    let pool = server::connect_pool(&database_url)
        .await
        .expect("failed to connect to database");

    server::run_migrations(&pool)
        .await
        .expect("failed to run migrations");

    let app = server::build_router(pool);

    println!("wallet service listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
