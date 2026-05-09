# Karyon p2p

A lightweight, extensible, and customizable peer-to-peer network stack.

## Features

- **Multi-transport**: TCP, TLS, and QUIC with priority-based
  endpoint selection
- **Pluggable custom protocols**: implement the `Protocol` trait, attach
  it to the node, and it runs over every peer connection alongside the
  built-in protocols
- **Pluggable discovery**: trait-based discovery abstraction with a
  built-in Kademlia DHT implementation
- **Multi-address peers**: each peer advertises connection and discovery
  addresses with protocol and priority
- **Async runtime flexibility**: supports both smol (default) and tokio

## Feature Flags

| Feature | Description |
|---------|-------------|
| `smol` | Use smol async runtime (default) |
| `tokio` | Use tokio async runtime |
| `quic` | Enable QUIC transport |
| `serde` | Enable serde support for p2p types |

```toml
# Default (smol + TCP/TLS)
karyon_p2p = "1.0"

# With QUIC
karyon_p2p = { version = "1.0", features = ["quic"] }

# With tokio
karyon_p2p = { version = "1.0", default-features = false, features = ["tokio", "quic"] }
```

## Architecture

```TEXT
                          Node
                            |
   +------------+-----------+-----------+
   |            |                       |
Discovery   PeerPool             Connector / Listener
   |            |                       |
Routing     Peers + Protocols       ConnQueue
table           |                       |
                +------- Handshake -----+
                             |
                          karyon_net (TCP / TLS / QUIC)
```

- **Node**: top-level lifecycle. Creates the discovery, pool, connector,
  and listener; wires them together; exposes the public API.
- **Discovery**: finds peers and yields them as `DiscoveredPeer`. Default
  is Kademlia DHT; custom implementations of the `Discovery` trait plug in.
- **Connector / Listener**: dial and accept connections over the
  configured transports, then hand them to `ConnQueue`.
- **ConnQueue**: short-lived bridge between accept/dial and handshake.
- **Handshake**: version + protocol negotiation. Verifies peer identity
  bound to the secure transport's certificate.
- **PeerPool**: registry of post-handshake peers. Owns the per-peer
  `Peer` state and broadcasts lifecycle events.
- **Peer / Protocol**: per-peer read loop dispatching to the registered
  protocols. Custom protocols implement the `Protocol` trait.

### Transport

Peers can listen and connect over multiple transports simultaneously.
Configuration uses endpoint URLs (`tcp://`, `tls://`, `quic://`); TLS
uses self-signed certs derived from the node's keypair, and QUIC
multiplexes one stream per protocol.

### Discovery

Implementations of the `Discovery` trait yield candidate peers; the
Node dials them. The default is a Kademlia DHT; plug in your own (mDNS,
static, ...) by implementing the trait.

```rust,ignore
#[async_trait]
pub trait Discovery: Send + Sync {
    async fn start(self: Arc<Self>) -> Result<()>;
    async fn shutdown(&self);
    async fn recv(&self) -> DiscoveredPeer;
    fn on_event(&self, event: PeerConnectionEvent);
    fn find_peers_with(&self, item: &[u8]) -> Vec<DiscoveredPeer>;
}
```

### Protocols

A built-in `Ping` keep-alive runs on every connection; everything else
is a custom protocol. For TCP/TLS, protocols share a single framed
connection. For QUIC, each protocol gets its own bidirectional stream.

`Protocol::kind()` defaults to `Optional`. Override to `Mandatory` for
protocols every peer must speak.

```rust
# use std::sync::Arc;
# use async_trait::async_trait;
# use karyon_p2p::{
#     protocol::{PeerConn, Protocol, ProtocolID},
#     Error, Version,
# };
pub struct MyProtocol {
    peer: PeerConn,
}

impl MyProtocol {
    fn new(peer: PeerConn) -> Arc<dyn Protocol> {
        Arc::new(Self { peer })
    }
}

#[async_trait]
impl Protocol for MyProtocol {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        loop {
            match self.peer.recv().await {
                Ok(_msg) => { /* handle */ }
                Err(Error::PeerShutdown) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn version() -> Result<Version, Error> {
        "0.1.0, >0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "MYPROTO".into()
    }

    // Optional override (defaults to ProtocolKind::Optional)
    // fn kind() -> ProtocolKind { ProtocolKind::Mandatory }
}
```

### Monitor

Subscribe to `node.monitor()` to observe what the network is doing -
connections opening and closing, peer add/remove, handshake failures,
discovery lookups and refreshes. Each topic is a separate event type
with its own listener; every subscriber gets every event independently.

```rust,no_run
# use karyon_core::async_runtime::global_executor;
# use karyon_p2p::{
#     monitor::ConnectionEvent,
#     keypair::{KeyPair, KeyPairType},
#     Node, Config,
# };
# async {
# let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
# let node = Node::new(&key_pair, Config::default(), global_executor());
let listener = node.monitor().register::<ConnectionEvent>();
while let Ok(ev) = listener.recv().await {
    println!("conn event: {} {:?}", ev.event, ev.endpoint);
}
# };
```

Available event types: `ConnectionEvent`, `PeerPoolEvent`,
`DiscoveryEvent`. Gated by `Config::enable_monitor`.

### Network Security

TLS is available for TCP connections. QUIC has built-in TLS 1.3. The
p2p layer generates self-signed certificates from the node's key pair
for mutual authentication.

## Example

```rust,no_run
use karyon_core::async_runtime::global_executor;
use karyon_p2p::{
    Node, Config,
    keypair::{KeyPair, KeyPairType},
};

async {
    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);

    let config = Config {
        listen_endpoints: vec![
            "tcp://0.0.0.0:8000".parse().unwrap(),
        ],
        discovery_endpoints: vec![
            "tcp://0.0.0.0:7000".parse().unwrap(),
            "udp://0.0.0.0:7000".parse().unwrap(),
        ],
        ..Config::default()
    };

    let node = Node::new(&key_pair, config, global_executor());

    node.run().await.expect("run node");

    // Register custom protocols, connect to peers, etc.

    node.shutdown().await;
};
```

## Examples

See [examples/](./examples) for a basic peer node and a chat application.
