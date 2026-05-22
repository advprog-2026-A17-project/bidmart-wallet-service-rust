use std::env;
use std::sync::Arc;

use bidmart_wallet_service_rust::persistence::repositories::WalletRepository;
use bidmart_wallet_service_rust::server;
use bidmart_wallet_service_rust::server::default_database_url;
use bidmart_wallet_service_rust::service::reconciliation::run_reconciliation_worker;
use dotenvy::from_path;

#[tokio::main]
async fn main() {
    let _ = from_path(".env");
    let _ = dotenvy::from_path_override("../bidmart-infrastructure/.env");

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| default_database_url());
    let port = env::var("PORT").unwrap_or_else(|_| "8083".to_string());
    let addr = format!("0.0.0.0:{port}");

    let pool = server::connect_pool(&database_url)
        .await
        .expect("failed to connect to database");

    server::run_migrations(&pool)
        .await
        .expect("failed to run migrations");

    let worker_repo = Arc::new(WalletRepository::new(pool.clone()));
    tokio::spawn(async move {
        run_reconciliation_worker(worker_repo).await;
    });

    let app = server::build_router(pool.clone());

    let grpc_port = env::var("GRPC_PORT").unwrap_or_else(|_| "50051".to_string());
    let grpc_addr = format!("0.0.0.0:{grpc_port}").parse().unwrap();
    let grpc_handler = bidmart_wallet_service_rust::grpc::WalletGrpcHandler::new(pool.clone());
    
    println!("wallet service listening on {addr}");
    println!("wallet gRPC service listening on {grpc_addr}");

    let http_server = axum::serve(tokio::net::TcpListener::bind(&addr).await.unwrap(), app);
    let grpc_server = tonic::transport::Server::builder()
        .add_service(bidmart_wallet_service_rust::grpc::WalletServiceServer::new(grpc_handler))
        .serve(grpc_addr);

    tokio::select! {
        res = http_server => {
            if let Err(e) = res {
                eprintln!("HTTP server error: {e}");
            }
        }
        res = grpc_server => {
            if let Err(e) = res {
                eprintln!("gRPC server error: {e}");
            }
        }
    }
}
