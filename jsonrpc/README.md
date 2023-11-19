# karyons jsonrpc

A fast and lightweight async [JSON-RPC 2.0](https://www.jsonrpc.org/specification) implementation.

## Example

```rust
use std::sync::Arc;

use serde_json::Value;

use karyons_jsonrpc::{JsonRPCError, Server, Client, register_service};

struct HelloWorld {}

impl HelloWorld {
    async fn say_hello(&self, params: Value) -> Result<Value, JsonRPCError> {
        let msg: String = serde_json::from_value(params)?;
        Ok(serde_json::json!(format!("Hello {msg}!")))
    }
}

//////////////////
// Server
//////////////////
let ex = Arc::new(smol::Executor::new());

// Creates a new server
let endpoint = "tcp://127.0.0.1:60000".parse().unwrap();
let server = Server::new_with_endpoint(&endpoint, ex.clone()).await.unwrap();

// Register the HelloWorld service
register_service!(HelloWorld, say_hello);
server.attach_service(HelloWorld{});

// Starts the server
ex.run(server.start());

//////////////////
// Client
//////////////////
// Creates a new client
let endpoint = "tcp://127.0.0.1:60000".parse().unwrap();
let client = Client::new_with_endpoint(&endpoint, None).await.unwrap();

let result: String = client.call("HelloWorld.say_hello", "world".to_string()).await.unwrap();

```
