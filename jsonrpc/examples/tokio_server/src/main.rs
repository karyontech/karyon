use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_jsonrpc::{
    error::RPCError,
    message::SubscriptionID,
    rpc_impl, rpc_pubsub_impl,
    server::{Channel, ServerBuilder},
};

struct Calc {
    version: String,
}

#[derive(Deserialize, Serialize)]
struct Req {
    x: u32,
    y: u32,
}

#[derive(Deserialize, Serialize)]
struct Pong {}

#[rpc_impl]
impl Calc {
    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!(Pong {}))
    }

    async fn add(&self, params: Value) -> Result<Value, RPCError> {
        let params: Req = serde_json::from_value(params)?;
        Ok(serde_json::json!(params.x + params.y))
    }

    async fn sub(&self, params: Value) -> Result<Value, RPCError> {
        let params: Req = serde_json::from_value(params)?;
        Ok(serde_json::json!(params.x - params.y))
    }

    async fn version(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!(self.version))
    }
}

#[rpc_pubsub_impl]
impl Calc {
    async fn log_subscribe(
        &self,
        chan: Arc<Channel>,
        method: String,
        _params: Value,
    ) -> Result<Value, RPCError> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if sub.notify(serde_json::json!("Hello")).await.is_err() {
                    break;
                }
            }
        });

        Ok(serde_json::json!(sub_id))
    }

    async fn log_unsubscribe(
        &self,
        chan: Arc<Channel>,
        _method: String,
        params: Value,
    ) -> Result<Value, RPCError> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        chan.remove_subscription(&sub_id).await;
        Ok(serde_json::json!(true))
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    // Register the Calc service
    let calc = Arc::new(Calc {
        version: String::from("0.1"),
    });

    // Creates a new server
    let server = ServerBuilder::new("ws://127.0.0.1:6000")
        .expect("Create a new server builder")
        .service(calc.clone())
        .pubsub_service(calc)
        .build()
        .await
        .expect("start a new server");

    // Start the server
    server.start();

    tokio::time::sleep(Duration::MAX).await;
}
