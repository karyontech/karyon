# karyon net

Async networking for Rust on tokio or smol: TCP, UDP, QUIC and Unix socket transports, composable
TLS (rustls), WebSocket and SOCKS5 layers, and codec-based message framing.

A connection is a byte stream, wrapped in optional layers, then framed with a codec into a message
connection. Bring your own transport to run over in-memory pipes, sockets from another runtime, or
a WASI host's sockets.

Use cases:

- Length-prefixed or custom-codec messaging over TCP, TLS, Unix sockets or QUIC streams.
- WebSocket clients and servers with an application codec.
- Multiplexed QUIC streams and unreliable datagrams.
- Dialing through a SOCKS5 proxy.
- Custom transports (in-memory, WASI) that reuse framing, TLS and SOCKS5.

## Install

```bash
$ cargo add karyon_net
```

## Architecture

```TEXT
                              Application
                                  |
           +----------------------+----------------------+
           |                                             |
     Stream Path                                    Mux Path
           |                                             |
   Box<dyn ByteStream>                         Box<dyn StreamMux>
           |                                         /        \
      FramedConn                              open_stream  accept_stream
      (+ codec)                                    |             |
      /      \                             Box<dyn ByteStream>
 recv_msg  send_msg                                |
      \      /                                FramedConn (+ codec)
   split() -> FramedReader + FramedWriter     /      \
                                         recv_msg  send_msg
```

**Layer composition**

```TEXT
    Application
        |
    FramedConn / WsConn   <- message framing (codec)
        |
    ByteStream            <- byte duplex (AsyncRead+Write)
        |
    +----+-----+--------+-----+
    | WS | TLS | SOCKS5 | ... |  <- optional layers (compose)
    +----+-----+--------+-----+
        |
    TCP / QUIC-stream / Unix / custom Transport
```

The byte stream comes from TCP, Unix, a QUIC `StreamMux`, or a custom `Transport`. TLS and SOCKS5
turn a `ByteStream` into a `ByteStream`, so they stack. WebSocket turns it into a `WsConn` and must
be the last layer.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `tokio` | Use tokio async runtime (default) |
| `smol` | Use smol async runtime |
| `all-protocols` | `tcp`, `tls`, `ws`, `udp`, `unix`, `proxy`, `quic` (default) |
| `tcp` | TCP transport |
| `tls` | TLS layer and `TlsListener` (implies `tcp`) |
| `ws` | WebSocket layer (implies `tcp`) |
| `quic` | QUIC transport (`StreamMux`) |
| `udp` | UDP datagram transport |
| `unix` | Unix domain socket transport |
| `proxy` | SOCKS5 proxy layer (implies `tcp`) |
| `framing` | `FramedConn` codec framing (enabled by `tcp`, `quic`, `unix`) |
| `serde` | serde support for `Endpoint` and `Addr` |
| `aws-lc-rs` | rustls crypto provider aws-lc-rs (default) |
| `ring` | rustls crypto provider ring |

`quic` needs one crypto provider. `tls` builds without one, but then every rustls config passed to
karyon_net must be built with an explicit provider (`ClientConfig::builder_with_provider`).

```toml
# Default (tokio + all protocols)
karyon_net = "2.5"

# Only TCP, TLS and the SOCKS5 proxy
karyon_net = { version = "2.5", default-features = false, features = ["tokio", "tls", "proxy", "aws-lc-rs"] }

# Smol runtime
karyon_net = { version = "2.5", default-features = false, features = ["smol", "tcp", "tls", "aws-lc-rs"] }

# ring instead of aws-lc-rs
karyon_net = { version = "2.5", default-features = false, features = ["tokio", "all-protocols", "ring"] }
```

## Core Traits and Types

- **ByteStream** - byte-oriented bidirectional stream (`AsyncRead + AsyncWrite`), with `peer_endpoint`, `local_endpoint` and `peer_certificates` (TLS, QUIC)
- **Codec\<W\>** - encodes and decodes messages from a wire type (`ByteBuffer` for byte streams, `ws::Message` for WebSocket). Built-in: `LengthCodec` (u32 length prefix), `BytesCodec` (raw bytes)
- **ClientLayer / ServerLayer** - composable async middleware (TLS, WebSocket, SOCKS5 proxy)
- **StreamMux** - multiplexed connection yielding multiple streams (QUIC)
- **FramedConn\<C\>** - message connection over a byte stream and codec: `recv_msg()`, `send_msg()`, `close()`, `split()`
- **FramedReader\<C\> / FramedWriter\<C\>** - split halves for concurrent use
- **WsConn\<C\>** - WebSocket message connection and codec, same API as `FramedConn`
- **WsReader\<C\> / WsWriter\<C\>** - split halves of a `WsConn`
- **MessageRx / MessageTx** - read and write traits implemented by the framed and WebSocket halves, for code generic over both
- **framed()** - creates a `FramedConn` from `Box<dyn ByteStream>` and a codec
- **Transport / TransportListener** - user supplied dial and listen, see [Custom Transport](#custom-transport)

**Closing a connection**

`FramedConn::close` and `FramedWriter::close` flush and shut down the write side. The read side
stays open.

`recv_msg` returns `Error::ConnectionClosed` when the peer closes the stream between frames, as
the WebSocket reader does. A close in the middle of a frame is an IO error of kind
`UnexpectedEof`.

## Layers

| Layer | Feature | Direction | Input | Output |
|-------|---------|-----------|-------|--------|
| TLS | `tls` | Client + Server | `Box<dyn ByteStream>` | `Box<dyn ByteStream>` |
| WebSocket | `ws` | Client + Server | `Box<dyn ByteStream>` | `WsConn<C>` |
| SOCKS5 | `proxy` | Client only | `Box<dyn ByteStream>` | `Box<dyn ByteStream>` |

## Usage

### TCP

```rust,no_run
# async fn _ex() -> karyon_net::Result<()> {
use karyon_net::{tcp, framed, codec::LengthCodec, Endpoint};

let ep: Endpoint = "tcp://127.0.0.1:8000".parse()?;
let stream = tcp::connect(&ep, Default::default()).await?;
let mut conn = framed(stream, LengthCodec::default());
conn.send_msg(Default::default()).await?;
let _msg = conn.recv_msg().await?;

// Split for concurrent read/write across tasks
let (mut _reader, mut _writer) = conn.split();
// reader.recv_msg() in one task, writer.send_msg() in another
# Ok(()) }
```

### TLS

```rust,no_run
# use karyon_net::tls::ClientTlsConfig;
# async fn _ex(tls_config: ClientTlsConfig) -> karyon_net::Result<()> {
use karyon_net::{tcp, framed, codec::LengthCodec, ClientLayer, Endpoint};
use karyon_net::tls::TlsLayer;

let ep: Endpoint = "tls://127.0.0.1:8000".parse()?;
let stream = tcp::connect(&ep, Default::default()).await?;
let tls_stream = ClientLayer::handshake(
    &TlsLayer::client(tls_config), stream
).await?;
let _conn = framed(tls_stream, LengthCodec::default());
# Ok(()) }
```

### WebSocket

`WsLayer` needs a codec that implements `Codec<ws::Message>`. There is no built-in one, since the
message kind depends on the application. The example shows the wiring with a user codec.
`WsLayer::with_max_msg_size` sets the max message size (4MB by default).

```rust,no_run
# use karyon_net::codec::Codec;
# use karyon_net::layers::ws::Message as WsMessage;
# #[derive(Clone)]
# struct AppCodec;
# impl Codec<WsMessage> for AppCodec {
#     type Message = WsMessage;
#     type Error = karyon_net::Error;
#     fn encode(&self, src: &WsMessage, dst: &mut WsMessage) -> Result<usize, Self::Error> {
#         *dst = src.clone();
#         Ok(0)
#     }
#     fn decode(&self, src: &mut WsMessage) -> Result<Option<(usize, WsMessage)>, Self::Error> {
#         Ok(Some((0, src.clone())))
#     }
# }
# async fn _ex() -> karyon_net::Result<()> {
use karyon_net::{tcp, ClientLayer, Endpoint, WsLayer};

let ep: Endpoint = "ws://127.0.0.1:8000".parse()?;
let stream = tcp::connect(&ep, Default::default()).await?;
let ws_layer = WsLayer::client("ws://127.0.0.1:8000", AppCodec);
let mut _ws_conn = ClientLayer::handshake(&ws_layer, stream).await?;
# Ok(()) }
```

### QUIC (multiplexed)

A `QuicConn` is a stream factory. It implements `StreamMux`, and also exposes `open_bi`,
`open_uni`, `accept_bi`, `accept_uni`, and unreliable datagrams (`send_datagram`,
`recv_datagram`, off by default, see `QuicConfig::enable_datagrams`). Transport parameters are set
with `with_config(QuicConfig)` on the server and client configs.

On the server, `QuicEndpoint::accept_incoming` returns a `QuicIncoming` whose `handshake` can run
in a separate task, so a slow peer cannot stall the accept loop. `QuicEndpoint::accept` runs the
handshake inline.

```rust,no_run
# use karyon_net::quic::ClientQuicConfig;
# async fn _ex(client_quic_config: ClientQuicConfig) -> karyon_net::Result<()> {
use karyon_net::{quic, framed, codec::LengthCodec, StreamMux, Endpoint};

let ep: Endpoint = "quic://127.0.0.1:9000".parse()?;
let quic_conn = quic::QuicEndpoint::dial(&ep, client_quic_config).await?;
let stream = quic_conn.open_stream().await?;
let _conn = framed(stream, LengthCodec::default());
# Ok(()) }
```

### SOCKS5 Proxy

```rust,no_run
# async fn _ex() -> karyon_net::Result<()> {
use karyon_net::{tcp, ClientLayer, Endpoint};
use karyon_net::proxy::Socks5Layer;

let proxy_ep: Endpoint = "tcp://127.0.0.1:1080".parse()?;
let stream = tcp::connect(&proxy_ep, Default::default()).await?;
let layer = Socks5Layer::new("example.com", 443);
let _tunneled = ClientLayer::handshake(&layer, stream).await?;
# Ok(()) }
```

### Listener

```rust,no_run
# async fn _ex() -> karyon_net::Result<()> {
use karyon_net::{tcp, framed, codec::LengthCodec, Endpoint};

let ep: Endpoint = "tcp://0.0.0.0:8000".parse()?;
let listener = tcp::TcpListener::bind(&ep, Default::default()).await?;

loop {
    let stream = listener.accept().await?;
    let _conn = framed(stream, LengthCodec::default());
    // conn.recv_msg(), conn.send_msg(), or conn.split()
}
# }
```

### TLS Listener

`TlsListener::accept` runs the handshake inline and fails after the handshake timeout (10s by
default, see `TlsListener::with_handshake_timeout`), so a slow client stalls a plain accept loop.
Busy servers should accept on a `TcpListener` and run the `TlsLayer` handshake in a separate task.

```rust,no_run
# use karyon_net::tls::ServerTlsConfig;
# async fn _ex(tls_config: ServerTlsConfig) -> karyon_net::Result<()> {
use karyon_net::{tcp, codec::LengthCodec, framed, Endpoint};
use karyon_net::tls::TlsListener;

let ep: Endpoint = "tls://0.0.0.0:8000".parse()?;
let tcp_listener = tcp::TcpListener::bind(&ep, Default::default()).await?;
let listener = TlsListener::new(tcp_listener, tls_config);

loop {
    let stream = listener.accept().await?;
    let _conn = framed(stream, LengthCodec::default());
}
# }
```

## Custom Transport

Implement `transport::Transport` to bring your own streams. It has three methods. `supports` tells
whether it handles an endpoint, `dial` returns a `Box<dyn ByteStream>`, and `listen` returns a
`Box<dyn TransportListener>` (`accept` and `local_endpoint`). Share it as `TransportRef`.

A transport can take a custom scheme (`mem://node-a` parses as `Endpoint::Custom`) or a built-in
one (`tcp://`, `tls://`, `quic://`). Crates on karyon_net, such as karyon_p2p and karyon_jsonrpc,
use it for every endpoint it supports and their built-in transports for the others.

The returned streams are used as is. No layer is added, but framing, TLS and SOCKS5 work on top.

`TcpListener` and `UnixListener` implement `TransportListener`, so a built-in listener and a user
transport's listener can be held as one `Box<dyn TransportListener>`.

See [examples/custom_transport.rs](examples/custom_transport.rs) for a `mem://` transport over
in-memory pipes with framed messages.

**Run the example:**

```bash
cargo run -p karyon_net --example custom_transport
```

## Endpoint URL Schemes

| Scheme | Transport |
|--------|-----------|
| `tcp://host:port` | Plain TCP |
| `tls://host:port` | TLS over TCP |
| `udp://host:port` | UDP |
| `ws://host:port` | WebSocket over TCP |
| `wss://host:port` | WebSocket over TLS |
| `quic://host:port` | QUIC |
| `http://host:port` | HTTP |
| `https://host:port` | HTTPS |
| `unix:path` | Unix domain socket |
| `scheme://addr` | Any other scheme, `Endpoint::Custom` for a user `Transport` |

## QUIC Runtime

By default QUIC uses quinn's runtime for the enabled feature. On smol, its connections then run on
smol's global executor, one thread by default.

`ServerQuicConfig::with_executor` and `ClientQuicConfig::with_executor` run the connections on a
karyon `Executor`. `with_runtime` takes a `QuicRuntimeRef` (`QuicRuntime` is quinn's `Runtime`:
tasks, timers and UDP sockets) to run QUIC on your own I/O reactor. It wins over `with_executor`.

## WASI

karyon_net builds for `wasm32-wasip1` and `wasm32-wasip2` with the `tokio` feature. TLS builds
without a provider feature if you pass a rustls `CryptoProvider` in its config, or enable `ring`
(aws-lc-rs does not build for wasm).

There are no OS sockets there. The TCP, UDP, Unix and QUIC transports and `TlsListener` are left
out, and the `ws` feature does not build. Framing, codecs, `Endpoint`, and the TLS and SOCKS5
layers work over streams from a custom `Transport`.

```toml
karyon_net = { version = "2.5", default-features = false, features = ["tokio", "tls", "ring"] }
```

**Run the custom transport example with wasmtime:**

```bash
CARGO_TARGET_WASM32_WASIP2_RUNNER=wasmtime cargo run -p karyon_net \
    --example custom_transport --target wasm32-wasip2 \
    --no-default-features --features tokio,framing
```
