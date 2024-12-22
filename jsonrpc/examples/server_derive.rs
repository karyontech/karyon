use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_core::async_util::sleep;
use karyon_jsonrpc::{error::RPCError, rpc_impl, rpc_method, server::ServerBuilder};

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

#[rpc_impl(name = "calculator")]
impl Calc {
    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!(Pong {}))
    }

    #[rpc_method(name = "math.add")]
    async fn add(&self, params: Value) -> Result<Value, RPCError> {
        let params: Req = serde_json::from_value(params)?;
        Ok(serde_json::json!(params.x + params.y))
    }

    #[rpc_method(name = "math.sub")]
    async fn sub(&self, params: Value) -> Result<Value, RPCError> {
        let params: Req = serde_json::from_value(params)?;
        Ok(serde_json::json!(params.x - params.y))
    }

    async fn version(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!(self.version))
    }
}

fn main() {
    env_logger::init();
    smol::block_on(async {
        // Register the Calc service
        let calc = Calc {
            version: String::from("0.1"),
        };

        // Creates a new server
        let server = ServerBuilder::new("tcp://127.0.0.1:6000")
            .expect("Create a new server builder")
            .service(Arc::new(calc))
            .build()
            .await
            .expect("start a new server");

        // Start the server
        server.start();

        sleep(Duration::MAX).await;
    });
}
