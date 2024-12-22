use std::{sync::Arc, time::Duration};

use log::error;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_core::async_util::sleep;
use karyon_jsonrpc::{
    error::RPCError,
    message::SubscriptionID,
    rpc_impl, rpc_pubsub_impl,
    server::{channel::Channel, ServerBuilder},
};

struct Calc {}

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
        let sub_id = sub.id.clone();
        smol::spawn(async move {
            loop {
                sleep(Duration::from_millis(500)).await;
                if let Err(err) = sub.notify(serde_json::json!("Hello")).await {
                    error!("Send notification {err}");
                    break;
                }
            }
        })
        .detach();

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

fn main() {
    env_logger::init();
    smol::block_on(async {
        let calc = Arc::new(Calc {});

        // Creates a new server
        let server = ServerBuilder::new("tcp://127.0.0.1:6000")
            .expect("Create a new server builder")
            .service(calc.clone())
            .pubsub_service(calc)
            .build()
            .await
            .expect("Build a new server");

        // Start the server
        server.start_block().await.expect("Start the server");
    });
}
