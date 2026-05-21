use std::env;
use std::sync::Arc;

use futures_lite::StreamExt;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicQosOptions, ExchangeDeclareOptions,
        QueueBindOptions, QueueDeclareOptions,
    },
    types::{AMQPValue, FieldTable},
    Connection, ConnectionProperties,
};
use serde::Deserialize;
use crate::service::wallet_service::WalletService;

#[derive(Debug, Deserialize)]
struct WalletProvisionPayload {
    #[serde(rename = "eventId")]
    event_id: String,
    #[serde(rename = "userId")]
    user_id: String,
    email: String,
    role: String,
    source: Option<String>,
}

pub struct WalletProvisioningConsumer {
    amqp_url: String,
    exchange: String,
    routing_key: String,
    queue_name: String,
    wallet_service: Arc<WalletService>,
}

impl WalletProvisioningConsumer {
    pub fn from_env(wallet_service: Arc<WalletService>) -> Self {
        Self {
            amqp_url: env::var("RABBITMQ_URL")
                .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".to_string()),
            exchange: env::var("WALLET_PROVISIONING_EXCHANGE")
                .unwrap_or_else(|_| "bidmart.wallet.provisioning".to_string()),
            routing_key: env::var("WALLET_PROVISIONING_ROUTING_KEY")
                .unwrap_or_else(|_| "wallet.provision.requested.v1".to_string()),
            queue_name: env::var("WALLET_PROVISIONING_QUEUE")
                .unwrap_or_else(|_| "wallet.provisioning.requests".to_string()),
            wallet_service,
        }
    }

    pub async fn run(self: Arc<Self>) {
        loop {
            if let Err(err) = self.consume_loop().await {
                eprintln!("wallet provisioning consumer error: {err}; retrying in 5s");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    async fn consume_loop(&self) -> Result<(), String> {
        let connection = Connection::connect(&self.amqp_url, ConnectionProperties::default())
            .await
            .map_err(|e| format!("AMQP connect: {e}"))?;

        let channel = connection
            .create_channel()
            .await
            .map_err(|e| format!("AMQP channel: {e}"))?;

        channel
            .basic_qos(1, BasicQosOptions::default())
            .await
            .map_err(|e| format!("AMQP qos: {e}"))?;

        channel
            .exchange_declare(
                &self.exchange,
                lapin::ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| format!("declare exchange: {e}"))?;

        channel
            .queue_declare(
                &self.queue_name,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| format!("declare queue: {e}"))?;

        channel
            .queue_bind(
                &self.queue_name,
                &self.exchange,
                &self.routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| format!("bind queue: {e}"))?;

        let mut consumer = channel
            .basic_consume(
                &self.queue_name,
                "wallet-provisioning-consumer",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| format!("consume: {e}"))?;

        while let Some(delivery) = consumer.next().await {
            let delivery = delivery.map_err(|e| format!("delivery: {e}"))?;
            if let Err(err) = self.handle_delivery(&delivery).await {
                eprintln!("wallet provisioning handler failed: {err}");
            }
            delivery
                .ack(BasicAckOptions::default())
                .await
                .map_err(|e| format!("ack: {e}"))?;
        }

        Err("consumer stream ended".to_string())
    }

    async fn handle_delivery(
        &self,
        delivery: &lapin::message::Delivery,
    ) -> Result<(), String> {
        let body = std::str::from_utf8(&delivery.data)
            .map_err(|e| format!("utf8 body: {e}"))?;
        let payload: WalletProvisionPayload =
            serde_json::from_str(body).map_err(|e| format!("json: {e}"))?;

        let source = delivery
            .properties
            .headers()
            .as_ref()
            .and_then(|headers| headers.inner().get("source"))
            .and_then(|value| match value {
                AMQPValue::LongString(s) => Some(s.to_string()),
                AMQPValue::ShortString(s) => Some(s.to_string()),
                _ => None,
            })
            .or(payload.source.clone())
            .unwrap_or_else(|| "bidmart-auth-service".to_string());

        self.wallet_service
            .provision_wallet(
                &payload.event_id,
                &payload.user_id,
                &payload.email,
                &payload.role,
                &source,
            )
            .await
            .map_err(|e| format!("provision_wallet: {e}"))?;

        Ok(())
    }
}
