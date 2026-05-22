pub mod wallet {
    tonic::include_proto!("wallet.v1");
}

use crate::service::wallet_service::WalletService;
use crate::wallet::Money;
use tonic::{Request, Response, Status};
use wallet::wallet_service_server::WalletService as GrpcWalletService;
pub use wallet::wallet_service_server::WalletServiceServer;
use wallet::{
    GrpcConvertFundsRequest, GrpcEmptyResponse, GrpcHoldFundsRequest, GrpcHoldFundsResponse,
    GrpcReleaseFundsRequest,
};

pub struct WalletGrpcHandler {
    wallet_service: WalletService,
}

impl WalletGrpcHandler {
    pub fn new(pool: sqlx::AnyPool) -> Self {
        Self {
            wallet_service: WalletService::new(pool),
        }
    }
}

#[tonic::async_trait]
impl GrpcWalletService for WalletGrpcHandler {
    async fn hold_funds(
        &self,
        request: Request<GrpcHoldFundsRequest>,
    ) -> Result<Response<GrpcHoldFundsResponse>, Status> {
        let req = request.into_inner();

        let user_id = &req.user_id;
        let role = req.role.as_deref().unwrap_or("BUYER");
        let hold_id = &req.hold_id;
        let auction_id = &req.auction_id;
        let bid_id = &req.bid_id;
        let expires_at = &req.expires_at;
        let amount = Money::from_rupiah(req.amount);

        match self
            .wallet_service
            .hold_funds(
                user_id, role, auction_id, bid_id, amount, hold_id, expires_at,
            )
            .await
        {
            Ok(hold) => Ok(Response::new(GrpcHoldFundsResponse {
                id: hold.id.to_string(),
                status: "HELD".to_string(),
                amount: hold.amount as u64,
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn release_hold(
        &self,
        request: Request<GrpcReleaseFundsRequest>,
    ) -> Result<Response<GrpcEmptyResponse>, Status> {
        let req = request.into_inner();
        let hold_id = &req.hold_id;

        match self.wallet_service.release_funds(hold_id).await {
            Ok(_) => Ok(Response::new(GrpcEmptyResponse {})),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn convert_hold_to_payment(
        &self,
        request: Request<GrpcConvertFundsRequest>,
    ) -> Result<Response<GrpcEmptyResponse>, Status> {
        let req = request.into_inner();
        let hold_id = &req.hold_id;

        match self.wallet_service.convert_funds(hold_id).await {
            Ok(_) => Ok(Response::new(GrpcEmptyResponse {})),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }
}
