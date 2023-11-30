use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use smol::net::TcpListener;

use karyons_jsonrpc::{register_service, JsonRPCError, Server, ServerConfig};

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

impl Calc {
    async fn ping(&self, _params: Value) -> Result<Value, JsonRPCError> {
        Ok(serde_json::json!(Pong {}))
    }

    async fn add(&self, params: Value) -> Result<Value, JsonRPCError> {
        let params: Req = serde_json::from_value(params)?;
        Ok(serde_json::json!(params.x + params.y))
    }

    async fn sub(&self, params: Value) -> Result<Value, JsonRPCError> {
        let params: Req = serde_json::from_value(params)?;
        Ok(serde_json::json!(params.x - params.y))
    }

    async fn version(&self, _params: Value) -> Result<Value, JsonRPCError> {
        Ok(serde_json::json!(self.version))
    }
}

fn main() {
    env_logger::init();
    let ex = Arc::new(smol::Executor::new());
    smol::block_on(ex.clone().run(async {
        // Creates a new server
        let listener = TcpListener::bind("127.0.0.1:60000").await.unwrap();
        let config = ServerConfig::default();
        let server = Server::new(listener.into(), config, ex);

        // Register the Calc service
        register_service!(Calc, ping, add, sub, version);
        let calc = Calc {
            version: String::from("0.1"),
        };
        server.attach_service(calc).await;

        // Start the server
        server.start().await.unwrap();
    }));
}
