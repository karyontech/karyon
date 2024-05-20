# karyon jsonrpc

A fast and lightweight async implementation of [JSON-RPC
2.0](https://www.jsonrpc.org/specification).

features: 
- Supports TCP, TLS, WebSocket, and Unix protocols.
- Uses smol(async-std) as the async runtime, but also supports tokio via the 
  `tokio` feature.
- Allows registration of multiple services (structs) of different types on a
  single server.

## Example

```rust
use std::sync::Arc;

use serde_json::Value;

use karyon_jsonrpc::{Error, Server, Client, rpc_impl};

struct HelloWorld {}

#[rpc_impl]
impl HelloWorld {
    async fn say_hello(&self, params: Value) -> Result<Value, Error> {
        let msg: String = serde_json::from_value(params)?;
        Ok(serde_json::json!(format!("Hello {msg}!")))
    }

    async fn foo(&self, params: Value) -> Result<Value, Error> {
        Ok(serde_json::json!("foo!"))
    }

    async fn bar(&self, params: Value) -> Result<Value, Error> {
        Ok(serde_json::json!("bar!"))
    }
}

// Server
async {
    // Creates a new server
    let server = Server::builder("tcp://127.0.0.1:60000")
        .expect("create new server builder")
        .service(HelloWorld{})
        .build()
        .await
        .expect("build the server");

    // Starts the server
    server.start().await.expect("start the server");
};

// Client
async {
    // Creates a new client
    let client = Client::builder("tcp://127.0.0.1:60000")
        .expect("create new client builder")
        .build()
        .await
        .expect("build the client");

    let result: String = client.call("HelloWorld.say_hello", "world".to_string())
        .await
        .expect("send a request");
};

```




