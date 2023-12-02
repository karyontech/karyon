# karyon jsonrpc

A fast and lightweight async implementation of [JSON-RPC
2.0](https://www.jsonrpc.org/specification), supporting the Tcp and Unix protocols.

## Example

```rust
use std::sync::Arc;

use serde_json::Value;
use smol::net::{TcpStream, TcpListener};

use karyon_jsonrpc::{JsonRPCError, Server, Client, register_service, ServerConfig, ClientConfig};

struct HelloWorld {}

impl HelloWorld {
    async fn say_hello(&self, params: Value) -> Result<Value, JsonRPCError> {
        let msg: String = serde_json::from_value(params)?;
        Ok(serde_json::json!(format!("Hello {msg}!")))
    }
}

let ex = Arc::new(smol::Executor::new());

//////////////////
// Server
//////////////////
// Creates a new server
let listener = TcpListener::bind("127.0.0.1:60000").await.unwrap();
let config = ServerConfig::default();
let server = Server::new(listener, config, ex.clone());

// Register the HelloWorld service
register_service!(HelloWorld, say_hello);
server.attach_service(HelloWorld{});

// Starts the server
ex.run(server.start());

//////////////////
// Client 
//////////////////
// Creates a new client
let conn = TcpStream::connect("127.0.0.1:60000").await.unwrap();
let config = ClientConfig::default();
let client = Client::new(conn, config);

let result: String = client.call("HelloWorld.say_hello", "world".to_string()).await.unwrap();

```
