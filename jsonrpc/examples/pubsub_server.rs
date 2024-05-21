use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_jsonrpc::{rpc_impl, rpc_pubsub_impl, ArcChannel, Error, Server, SubscriptionID};

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
    async fn ping(&self, _params: Value) -> Result<Value, Error> {
        Ok(serde_json::json!(Pong {}))
    }
}

#[rpc_pubsub_impl]
impl Calc {
    async fn log_subscribe(&self, chan: ArcChannel, _params: Value) -> Result<Value, Error> {
        let sub = chan.new_subscription().await;
        let sub_id = sub.id.clone();
        smol::spawn(async move {
            loop {
                smol::Timer::after(std::time::Duration::from_secs(1)).await;
                if let Err(err) = sub.notify(serde_json::json!("Hello")).await {
                    println!("Error send notification {err}");
                    break;
                }
            }
        })
        .detach();

        Ok(serde_json::json!(sub_id))
    }

    async fn log_unsubscribe(&self, chan: ArcChannel, params: Value) -> Result<Value, Error> {
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
        let server = Server::builder("tcp://127.0.0.1:6000")
            .expect("Create a new server builder")
            .service(calc.clone())
            .pubsub_service(calc)
            .build()
            .await
            .expect("Build a new server");

        // Start the server
        server.start().await.expect("Start the server");
    });
}
