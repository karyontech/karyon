# karyon jsonrpc

An async [JSON-RPC 2.0](https://www.jsonrpc.org/specification) server and client library for
Rust, with pub/sub subscriptions, attribute macros for services, and pluggable codecs.

It runs over TCP, TLS, WebSocket, WSS, QUIC, HTTP/1.1, HTTP/2, HTTP/3, and Unix sockets, on tokio
or smol. Bring your own transport and runtime to run it in custom environments, including WASI and
executors other than tokio and smol.

Use cases:

- RPC APIs for daemons, nodes, and backend services.
- Pushing events to clients through subscriptions, over any transport except HTTP/1.1 and HTTP/2.
- Local IPC over Unix sockets.
- RPC inside WASI hosts or over in-memory pipes, through a custom transport.

## Install

```bash
$ cargo add karyon_jsonrpc
```

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

## Services

`#[rpc_impl]` implements `RPCService` for a type. Every method in the block becomes an RPC method.
It takes `&self` and a `serde_json::Value` and returns `Result<Value, RPCError>`.

`#[rpc_pubsub_impl]` implements `PubSubRPCService`. Its methods take `&self`, the client's
`Arc<Channel>`, the method name as `String`, and the params as `Value`. `Channel::new_subscription`
creates a subscription and `Subscription::notify` sends it a notification.

Clients call service methods as `<service>.<method>`. The service name defaults to the type name.
Rename it with `#[rpc_impl(name = "calculator")]` and a method with `#[rpc_method(name = "math.add")]`.
See [examples/server_derive.rs](examples/server_derive.rs).

Register services with `ServerBuilder::service` and `ServerBuilder::pubsub_service`. Implementing
`RPCService` by hand also works, see [examples/server.rs](examples/server.rs).

### Standalone methods

Use `ServerBuilder::method` to register an async handler directly. Clients use the exact registered
name, which can be unqualified (such as `echo`) or contain dots (such as `health.ping`).

```rust,no_run
use karyon_jsonrpc::server::ServerBuilder;

async {
    let server = ServerBuilder::new("tcp://127.0.0.1:60000")
        .expect("create server builder")
        .method("echo", |params| async { Ok(params) })
        .method("health.ping", |_| async { Ok(serde_json::json!("pong")) })
        .build()
        .await
        .expect("build the server");

    server.start_block().await.expect("start the server");
};
```

If a standalone name also resolves to a registered RPC service method, `build()` returns an error
before the server starts listening.

## Endpoints

The endpoint URL passed to `ServerBuilder::new` or `ClientBuilder::new` selects the transport.

| Endpoint | Transport | Builder config |
|----------|-----------|----------------|
| `tcp://` | TCP | `tcp_config` (optional) |
| `tls://` | TLS over TCP | `tls_config` (required), `tcp_config` |
| `ws://` | WebSocket | `tcp_config` (optional) |
| `wss://` | WebSocket over TLS | `tls_config` (required), `tcp_config` |
| `quic://` | QUIC | `quic_config` (required) |
| `http://` | HTTP/1.1, HTTP/2, HTTP/3 | `http_config`, plus `quic_config` for HTTP/3 |
| `unix://` | Unix socket, Unix targets only | |

TCP, TLS, WebSocket, and Unix use one framed connection per client, shared by concurrent calls
and subscriptions. The connection is split into a reader and a writer (`FramedReader` and
`FramedWriter`), so requests and responses flow concurrently. Responses are matched by request id.
QUIC opens a bidirectional stream per call through `StreamMux`. A subscription's stream carries its
notifications and its unsubscribe request.

**HTTP**

The server accepts HTTP/1.1 and HTTP/2 `POST` requests on one TCP port, with bodies up to 1 MB.
With `ServerBuilder::quic_config` it also serves HTTP/3 on the same port number.

The client uses HTTP/1.1 by default. Set `HttpConfig { http2: true, .. }` with
`ClientBuilder::http_config` for HTTP/2 over plain TCP (prior knowledge, one shared connection).
`ClientBuilder::quic_config` switches to HTTP/3 and takes priority over `http2`. `HttpConfig::host`
sets the `Host` header for HTTP/1.1.

Subscriptions work over HTTP/3 only, with notifications streamed as DATA frames on the open
response. Over HTTP/1.1 and HTTP/2, `subscribe` returns `Error::UnsupportedProtocol`.

QUIC and HTTP/3 connections run on the executor set with `with_executor`, or the global one if none
is set. A runtime set with `with_runtime` on the QUIC config takes priority.

## Configuration

Client:

- `set_timeout`: request timeout in milliseconds.
- `set_max_subscription_buffer_size`: notifications buffered per subscription.
- `with_notification_decoder`: decode notifications sent in a custom format.

Server:

- `read_timeout`: drop a connection that sends nothing for this long. Off by default, since pubsub
  subscribers idle by design.
- `handshake_timeout`: drop a client that does not finish the TLS or WebSocket handshake in time.
  Defaults to 10 seconds.
- `with_notification_encoder`: send notifications in a custom format. A karyon client then needs the
  matching `with_notification_decoder`.

## Codecs

`JsonCodec` is the default, with a 4 MB max payload. `JsonCodec::new` sets another limit.

For another wire format, implement `Codec<ByteBuffer>` with `Message = serde_json::Value` and
`Error = karyon_net::Error`. The type must also be `Clone + Send + Sync + 'static`, which makes it
a `codec::JsonRpcCodec`. Pass it to `ServerBuilder::new_with_codec` or `ClientBuilder::new_with_codec`. It is used for TCP, TLS,
Unix, QUIC, and HTTP. WebSocket endpoints use a separate codec over WebSocket messages, set with
`with_ws_codec`. Over HTTP, set `HttpConfig::content_type` to match the codec.

See [examples/server_custom_codec.rs](examples/server_custom_codec.rs) and
[examples/client_custom_codec.rs](examples/client_custom_codec.rs).

## Feature Flags

All features except `smol` and `ring` are enabled by default.

| Feature | Description |
|---------|-------------|
| `tokio` | tokio async runtime |
| `smol` | smol async runtime |
| `tcp` | TCP |
| `tls` | TLS over TCP (implies `tcp`) |
| `ws` | WebSocket over TCP (implies `tcp`) |
| `quic` | QUIC, needs a crypto provider feature |
| `http` | HTTP/1.1 and HTTP/2 over TCP (implies `tcp`) |
| `http3` | HTTP/3 over QUIC (implies `http` and `quic`) |
| `unix` | Unix socket |
| `aws-lc-rs` | rustls crypto provider for TLS and QUIC: aws-lc-rs |
| `ring` | rustls crypto provider for TLS and QUIC: ring |

Enable exactly one runtime. To pick a subset, disable the default features:

```toml
# TCP only, on tokio
karyon_jsonrpc = { version = "1.4", default-features = false, features = ["tokio", "tcp"] }

# QUIC on smol, with ring
karyon_jsonrpc = { version = "1.4", default-features = false, features = ["smol", "quic", "ring"] }
```

## Custom Transport

The client and server dial and listen with karyon_net's built-in transports. To bring your own
streams (in-memory pipes, sockets from another runtime, a WASI host), implement
`karyon_net::transport::Transport` (`supports`, `dial`, `listen`) and pass it to
`ClientBuilder::with_transport` and `ServerBuilder::with_transport`.

Endpoints the transport supports, e.g. a custom `mem://calc`, go through it, with messages framed by
the codec. The rest use the built-in transports.

```rust
use karyon_jsonrpc::{client::ClientBuilder, server::ServerBuilder};
use karyon_net::transport::TransportRef;
# use karyon_jsonrpc::error::Result;

# async fn run(transport: TransportRef) -> Result<()> {
let server = ServerBuilder::new("mem://calc")?
    .with_transport(transport.clone())
    .build()
    .await?;
server.clone().start();

let client = ClientBuilder::new("mem://calc")?
    .with_transport(transport)
    .build()
    .await?;
# let _ = client;
# Ok(())
# }
```

## Custom Runtime

To use your own executor, implement `karyon_core::async_runtime::Runtime` (`schedule` a task,
`sleep`) and pass it as `Executor::from(runtime)`. The client or server tasks, call and read
timeouts, handshakes, and accept backoff then run on it.

```rust
use karyon_core::async_runtime::{Executor, RuntimeRef};
use karyon_jsonrpc::client::ClientBuilder;
# use karyon_jsonrpc::error::Result;

# async fn run(runtime: RuntimeRef) -> Result<()> {
let client = ClientBuilder::new("tcp://127.0.0.1:6000")?
    .with_executor(Executor::from(runtime))
    .build()
    .await?;
# let _ = client;
# Ok(())
# }
```

`ServerBuilder::with_executor` works the same way.

## WASI

karyon_jsonrpc supports `wasm32-wasip2` and `wasm32-wasip1` with only the `tokio` feature. The
socket features (`tcp`, `unix`, `ws`, `quic`, `http`) are a compile error there. Provide your own
`Runtime` and `Transport` as described above.

```toml
karyon_jsonrpc = { version = "1.4", default-features = false, features = ["tokio"] }
```

Run the custom runtime example with wasmtime:

```bash
CARGO_TARGET_WASM32_WASIP2_RUNNER=wasmtime cargo run -p karyon_jsonrpc \
    --example custom_runtime --target wasm32-wasip2 \
    --no-default-features --features tokio
```

## Examples

- [examples/server.rs](examples/server.rs): TCP server with a hand-written `RPCService`.
- [examples/server_derive.rs](examples/server_derive.rs),
  [examples/client_derive.rs](examples/client_derive.rs): renamed service and methods.
- [examples/http_server.rs](examples/http_server.rs): HTTP server.
  [examples/http_client.py](examples/http_client.py) calls it over HTTP/1.1 and HTTP/2.
- [examples/server_custom_codec.rs](examples/server_custom_codec.rs),
  [examples/client_custom_codec.rs](examples/client_custom_codec.rs): custom codec.
- [examples/custom_runtime.rs](examples/custom_runtime.rs): a server and a client on a
  single-threaded runtime, connected through in-memory pipes. Also runs on WASI.
- [examples/client.py](examples/client.py): raw TCP client in Python.

## Clients

Any JSON-RPC 2.0 client works with the server. A Go client is available at
[karyon-jsonrpc-go](https://github.com/karyontech/karyon-jsonrpc-go).
