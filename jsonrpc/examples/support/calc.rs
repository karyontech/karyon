//! A server and a client on one executor, connected by `MemTransport`:
//! the client calls `Calc.add` and gets the sum back. Shared by the
//! examples, which differ only in the runtime behind the executor.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use karyon_core::async_runtime::Executor;
use karyon_jsonrpc::{client::ClientBuilder, error::RPCError, rpc_impl, server::ServerBuilder};
use karyon_net::transport::TransportRef;

use crate::mem_transport::MemTransport;

const ENDPOINT: &str = "mem://calc";

struct Calc;

#[derive(Deserialize, Serialize)]
struct AddReq {
    x: u32,
    y: u32,
}

#[rpc_impl]
impl Calc {
    async fn add(&self, params: Value) -> Result<Value, RPCError> {
        let req: AddReq = serde_json::from_value(params)?;
        Ok(json!(req.x + req.y))
    }
}

/// Starts the server, calls `Calc.add(x, y)` from a client, and stops both.
pub async fn add(ex: Executor, x: u32, y: u32) -> u32 {
    let transport: TransportRef = Arc::new(MemTransport::new());

    let server = ServerBuilder::new(ENDPOINT)
        .expect("server builder")
        .service(Arc::new(Calc))
        .with_transport(transport.clone())
        .with_executor(ex.clone())
        .build()
        .await
        .expect("build the server");
    server.clone().start();

    let client = ClientBuilder::new(ENDPOINT)
        .expect("client builder")
        .with_transport(transport)
        .with_executor(ex)
        .build()
        .await
        .expect("connect the client");

    let sum = client
        .call("Calc.add", AddReq { x, y })
        .await
        .expect("call Calc.add");

    client.stop().await;
    server.shutdown().await;
    sum
}
