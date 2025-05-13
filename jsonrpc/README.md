# karyon jsonrpc

A fast and lightweight async implementation of [JSON-RPC
2.0](https://www.jsonrpc.org/specification).

features: 
- Supports TCP, TLS, WebSocket, and Unix protocols.
- Uses `smol`(async-std) as the async runtime, with support for `tokio` via the 
  `tokio` feature.
- Enables the registration of multiple services (structs) on a single server.
- Offers support for custom JSON codec.
- Includes support for pub/sub.  
- Allows the use of an `async_executors::Executor` or `tokio::Runtime` when building
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
    error::RPCError, server::{Server, ServerBuilder, Channel}, client::ClientBuilder,
    rpc_impl, rpc_pubsub_impl, rpc_method, message::SubscriptionID,
};

struct HelloWorld {}

// It is possible to change the service name by adding a `name` attribute 
#[rpc_impl]
impl HelloWorld {
    async fn say_hello(&self, params: Value) -> Result<Value, RPCError> {
        let msg: String = serde_json::from_value(params)?;
        Ok(serde_json::json!(format!("Hello {msg}!")))
    }

    #[rpc_method(name = "foo_method")]
    async fn foo(&self, params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!("foo!"))
    }

    async fn bar(&self, params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!("bar!"))
    }
}


// It is possible to change the service name by adding a `name` attribute 
#[rpc_pubsub_impl]
impl HelloWorld {
    async fn log_subscribe(&self, chan: Arc<Channel>, method: String, _params: Value) -> Result<Value, RPCError> {
        let sub = chan.new_subscription(&method, None).await.expect("Failed to subscribe");
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

    let server = ServerBuilder::new("tcp://127.0.0.1:60000")
        .expect("create new server builder")
        .service(service.clone())
        .pubsub_service(service)
        .build()
        .await
        .expect("build the server");

    // Starts the server
    server.start_block()
        .await
        .expect("Start the server");

};

// Client
async {
    // Creates a new client
    let client = ClientBuilder::new("tcp://127.0.0.1:60000")
        .expect("create new client builder")
        .build()
        .await
        .expect("build the client");

    let result: String = client.call("HelloWorld.say_hello", "world".to_string())
        .await
        .expect("send a request");

    let result: String = client.call("HelloWorld.foo_method", ())
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


