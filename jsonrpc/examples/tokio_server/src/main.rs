use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_jsonrpc::{rpc_impl, Error, Server};

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
    async fn ping(&self, _params: Value) -> Result<Value, Error> {
        Ok(serde_json::json!(Pong {}))
    }

    async fn add(&self, params: Value) -> Result<Value, Error> {
        let params: Req = serde_json::from_value(params)?;
        Ok(serde_json::json!(params.x + params.y))
    }

    async fn sub(&self, params: Value) -> Result<Value, Error> {
        let params: Req = serde_json::from_value(params)?;
        Ok(serde_json::json!(params.x - params.y))
    }

    async fn version(&self, _params: Value) -> Result<Value, Error> {
        Ok(serde_json::json!(self.version))
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    // Register the Calc service
    let calc = Calc {
        version: String::from("0.1"),
    };

    // Creates a new server
    let server = Server::builder("tcp://127.0.0.1:6000")
        .expect("Create a new server builder")
        .service(calc)
        .build()
        .await
        .expect("start a new server");

    // Start the server
    server.start().await.unwrap();
}
