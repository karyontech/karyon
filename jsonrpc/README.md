# karyon jsonrpc

A fast and lightweight async implementation of [JSON-RPC
2.0](https://www.jsonrpc.org/specification).

features: 
- Supports TCP, TLS, WebSocket, and Unix protocols.
- Uses `smol`(async-std) as the async runtime, but also supports `tokio` via the 
  `tokio` feature.
- Allows registration of multiple services (structs) of different types on a
  single server.
- Supports pub/sub  
- Allows passing an `async_executors::Executor` or tokio's `Runtime` when building
  the server.


## Install 

```bash
    
$ cargo add karyon_jsonrpc 

```

## Example

```rust
use std::{sync::Arc, time::Duration};

use serde_json::Value;
use smol::stream::StreamExt;

use karyon_jsonrpc::{
    RPCError, Server, Client, rpc_impl, rpc_pubsub_impl, SubscriptionID, Channel
};

struct HelloWorld {}

#[rpc_impl]
impl HelloWorld {
    async fn say_hello(&self, params: Value) -> Result<Value, RPCError> {
        let msg: String = serde_json::from_value(params)?;
        Ok(serde_json::json!(format!("Hello {msg}!")))
    }

    async fn foo(&self, params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!("foo!"))
    }

    async fn bar(&self, params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!("bar!"))
    }
}

#[rpc_pubsub_impl]
impl HelloWorld {
    async fn log_subscribe(&self, chan: Arc<Channel>, method: String, _params: Value) -> Result<Value, RPCError> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id.clone();
        smol::spawn(async move {
            loop {
                smol::Timer::after(std::time::Duration::from_secs(1)).await;
                if let Err(err) = sub.notify(serde_json::json!("Hello")).await {
                    println!("Failed to send notification: {err}");
                    break;
                }
            }
        })
        .detach();

        Ok(serde_json::json!(sub_id))
    }

    async fn log_unsubscribe(&self, chan: Arc<Channel>, method: String, params: Value) -> Result<Value, RPCError> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        chan.remove_subscription(&sub_id).await;
        Ok(serde_json::json!(true))
    }
}


// Server
async {
    let service = Arc::new(HelloWorld {});
    // Creates a new server

    let server = Server::builder("tcp://127.0.0.1:60000")
        .expect("create new server builder")
        .service(service.clone())
        .pubsub_service(service)
        .build()
        .await
        .expect("build the server");

    // Starts the server
    server.start();

    smol::Timer::after(Duration::MAX).await;
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

    let sub = client
            .subscribe("HelloWorld.log_subscribe", ())
            .await
            .expect("Subscribe to log_subscribe method");

    let sub_id = sub.id();
    smol::spawn(async move {
        loop {
            let m = sub.recv().await.expect("Receive new log msg");
            println!("Receive new log {m}");
        }
    })
    .detach();

    // Unsubscribe after 5 seconds
    smol::Timer::after(std::time::Duration::from_secs(5)).await;

    client
        .unsubscribe("HelloWorld.log_unsubscribe", sub_id)
        .await
        .expect("Unsubscribe from log_unsubscirbe method");
};

```

## Supported Client Implementations 

- [X] [Golang](https://github.com/karyontech/karyon-go)
- [ ] Python 
- [ ] JavaScript/TypeScript 


