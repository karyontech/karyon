# karyon net

A layered async networking stack over TCP, QUIC, UDP, and Unix
sockets, with composable layers for TLS, WebSocket, and SOCKS5.

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

### Layer Composition

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
    TCP / QUIC-stream / Unix
```

Construction is bottom-up: start with a transport (TCP, Unix, or a stream from
a QUIC `StreamMux`), wrap it in any number of optional layers, then apply a
codec to get a message connection. `TLS` and `SOCKS5` transform `ByteStream`
into `ByteStream` and can stack, with the exception for WebSocket which can't
have another layer added on top.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `tcp` | TCP transport |
| `tls` | TLS layer (implies `tcp`) |
| `ws` | WebSocket layer (implies `tcp`) |
| `quic` | QUIC transport (StreamMux) |
| `udp` | UDP datagram transport |
| `unix` | Unix domain socket transport |
| `proxy` | SOCKS5 proxy layer (implies `tcp`) |
| `framing` | FramedConn codec framing (auto-enabled by `tcp`, `quic`, `unix`) |
| `smol` | Use smol async runtime (default) |
| `tokio` | Use tokio async runtime |

```toml
# Default (smol + all protocols)
karyon_net = "1.0"

# With QUIC and proxy
karyon_net = { version = "1.0", features = ["quic", "proxy"] }

# Tokio runtime
karyon_net = { version = "1.0", default-features = false, features = ["tokio", "tcp", "tls"] }
```

## Core Traits and Types

- **ByteStream** - byte-oriented bidirectional stream (`AsyncRead + AsyncWrite`)
- **Codec\<W\>** - encodes/decodes messages from a wire type (`ByteBuffer` for byte streams, `ws::Message` for WebSocket)
- **ClientLayer / ServerLayer** - composable async middleware (TLS, WebSocket, SOCKS5 proxy)
- **StreamMux** - multiplexed connection yielding multiple streams (QUIC)
- **FramedConn\<C\>** - message connection over a byte stream + codec; `recv_msg()`, `send_msg()`, `split()`
- **FramedReader\<C\> / FramedWriter\<C\>** - split halves for concurrent use 
- **WsConn\<C\>** - WebSocket message connection + codec; same API as FramedConn
- **WsReader\<C\> / WsWriter\<C\>** - split halves of a WsConn
- **framed()** - creates a `FramedConn` from `Box<dyn ByteStream>` + codec

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

`WsLayer` needs a codec that implements `Codec<ws::Message>`. There's no
built-in one (since the message kind depends on the application), so the
caller provides their own; here is the wiring pattern.

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

## Runtime Support

Supports both **smol** (default) and **tokio** async runtimes.
