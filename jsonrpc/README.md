# karyon jsonrpc

A fast and lightweight async implementation of [JSON-RPC
2.0](https://www.jsonrpc.org/specification). Runs over TCP, TLS,
WebSocket, WSS, QUIC, HTTP/1.1, HTTP/2, HTTP/3, and Unix sockets,
with pub/sub and custom JSON codecs.

## Install

```bash
$ cargo add karyon_jsonrpc
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `tcp` | TCP (included by default) |
| `tls` | TLS over TCP (implies `tcp`) |
| `ws` | WebSocket over TCP (implies `tcp`) |
| `quic` | QUIC |
| `http` | HTTP/1.1 and HTTP/2 over TCP (implies `tcp`) |
| `http3` | HTTP/3 over QUIC (implies `http` and `quic`) |
| `unix` | Unix socket (included by default) |
| `smol` | Use smol async runtime (default) |
| `tokio` | Use tokio async runtime |

```toml
# QUIC support
karyon_jsonrpc = { version = "1.0", features = ["quic"] }

# HTTP/1.1 + HTTP/2
karyon_jsonrpc = { version = "1.0", features = ["http"] }

# HTTP/3 (includes HTTP/1.1+2 fallback and QUIC)
karyon_jsonrpc = { version = "1.0", features = ["http3"] }

# All wire formats
karyon_jsonrpc = { version = "1.0", features = ["tcp", "tls", "ws", "quic", "http3", "unix"] }
```

## Architecture

```TEXT
  ClientBuilder / ServerBuilder
           |
     endpoint dispatch
           |
  +--------+--------+--------+--------+
  | TCP    | TLS    | QUIC   | HTTP   |
  +--------+--------+--------+--------+
  |        |        |        |        |
  | framed | framed | stream | hyper  |
  | conn   | conn   | per    | / h3   |
  |        |        | call   |        |
  +--------+--------+--------+--------+
           |
     FramedReader / FramedWriter
     (concurrent recv_msg / send_msg)
```

- **TCP/TLS/Unix**: `tcp::connect()` or `TcpListener::bind()`, optionally wrapped with
  `TlsLayer`, then `framed()` to get a `FramedConn`. The connection is split into
  `FramedReader` + `FramedWriter` for concurrent request reading and response writing.
- **QUIC**: each RPC call opens a new bidirectional stream via `StreamMux`. Each subscription
  also opens its own dedicated stream and splits it for concurrent notification writing +
  unsubscribe reading.
- **HTTP/1-2**: standard hyper request/response. No persistent connection.
- **HTTP/3**: QUIC-based. Subscriptions stream DATA frames on a single request.

## Example

### Server

```rust
use std::sync::Arc;

use serde_json::Value;

use karyon_jsonrpc::{
    error::RPCError, rpc_impl, server::ServerBuilder,
};

struct Calc {}

#[rpc_impl]
impl Calc {
    async fn add(&self, params: Value) -> Result<Value, RPCError> {
        let (a, b): (i32, i32) = serde_json::from_value(params)?;
        Ok(serde_json::json!(a + b))
    }

    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!("pong"))
    }
}

async {
    let service = Arc::new(Calc {});

    let server = ServerBuilder::new("tcp://127.0.0.1:60000")
        .expect("create server builder")
        .service(service)
        .build()
        .await
        .expect("build the server");

    server.start_block().await.expect("start the server");
};
```

### Client

```rust
use karyon_jsonrpc::client::ClientBuilder;

async {
    let client = ClientBuilder::new("tcp://127.0.0.1:60000")
        .expect("create client builder")
        .build()
        .await
        .expect("build the client");

    let result: i32 = client
        .call("Calc.add", (1, 2))
        .await
        .expect("call add");
};
```

### Pub/Sub

```rust
use std::sync::Arc;

use serde_json::Value;

use karyon_jsonrpc::{
    error::RPCError, rpc_impl, rpc_pubsub_impl,
    message::SubscriptionID,
    server::{channel::Channel, ServerBuilder},
    client::ClientBuilder,
};

struct MyService {}

#[rpc_impl]
impl MyService {
    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!("pong"))
    }
}

#[rpc_pubsub_impl]
impl MyService {
    async fn log_subscribe(
        &self,
        chan: Arc<Channel>,
        method: String,
        _params: Value,
    ) -> Result<Value, RPCError> {
        let sub = chan
            .new_subscription(&method, None)
            .await
            .map_err(|e| RPCError::InvalidRequest(e.to_string()))?;
        let sub_id = sub.id();
        Ok(serde_json::json!(sub_id))
    }

    async fn log_unsubscribe(
        &self,
        chan: Arc<Channel>,
        _method: String,
        params: Value,
    ) -> Result<Value, RPCError> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        let ok = chan.remove_subscription(&sub_id).await.is_ok();
        Ok(serde_json::json!(ok))
    }
}

async {
    let service = Arc::new(MyService {});

    // Server with pubsub
    let server = ServerBuilder::new("ws://127.0.0.1:60000")
        .expect("create server builder")
        .service(service.clone())
        .pubsub_service(service)
        .build()
        .await
        .expect("build the server");

    server.clone().start();

    // Client subscribes
    let client = ClientBuilder::new("ws://127.0.0.1:60000")
        .expect("create client builder")
        .build()
        .await
        .expect("build the client");

    let sub = client
        .subscribe("MyService.log_subscribe", ())
        .await
        .expect("subscribe");

    // Receive notifications
    let msg = sub.recv().await.expect("receive notification");

    // Unsubscribe
    client
        .unsubscribe("MyService.log_unsubscribe", sub.id())
        .await
        .expect("unsubscribe");
};
```

## Clients

The server speaks standard JSON-RPC 2.0, so any compliant client works.
A Go client is available at [karyon-jsonrpc-go](https://github.com/karyontech/karyon-jsonrpc-go).
