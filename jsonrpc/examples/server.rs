use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_jsonrpc::{
    error::RPCError,
    server::{RPCMethod, RPCService, ServerBuilder},
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

impl RPCService for Calc {
    fn get_method(&self, name: &str) -> Option<RPCMethod> {
        match name {
            "ping" => Some(Box::new(move |params: Value| Box::pin(self.ping(params)))),
            "version" => Some(Box::new(move |params: Value| {
                Box::pin(self.version(params))
            })),
            _ => unimplemented!(),
        }
    }

    fn name(&self) -> String {
        "Calc".to_string()
    }
}

impl Calc {
    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!(Pong {}))
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
        let server = ServerBuilder::new("tcp://127.0.0.1:7878")
            .expect("Create a new server builder")
            .service(Arc::new(calc))
            .build()
            .await
            .expect("start a new server");

        // Start the server
        server.start_block().await.expect("Start the server");
    });
}
