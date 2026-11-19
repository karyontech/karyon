# Karyon p2p

A lightweight, extensible, and customizable peer-to-peer network stack.
Multi-transport (TCP, TCP/TLS, QUIC) with pluggable custom protocols via the
`Protocol` trait, and pluggable discovery. A Kademlia DHT discovery is in
[karyon_kademlia](https://crates.io/crates/karyon_kademlia).

## Install

```bash
$ cargo add karyon_p2p
```

## Example

An echo node: it attaches a custom protocol that broadcasts back every
message it receives.

```rust
use std::sync::Arc;

use async_trait::async_trait;

use karyon_core::async_runtime::global_executor;
use karyon_p2p::{
    keypair::{KeyPair, KeyPairType},
    protocol::{PeerConn, Protocol, ProtocolID},
    Config, Error, Node, Version,
};

pub struct EchoProtocol {
    peer: PeerConn,
}

impl EchoProtocol {
    fn new(peer: PeerConn) -> Self {
        Self { peer }
    }
}

#[async_trait]
impl Protocol for EchoProtocol {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        loop {
            match self.peer.recv().await {
                Ok(msg) => self.peer.broadcast(msg).await,
                Err(Error::PeerShutdown) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn version() -> Result<Version, Error> {
        "0.1.0, 0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "ECHO".into()
    }
}

async {
    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);

    let config = Config {
        listen_endpoints: vec!["tcp://0.0.0.0:8000".parse().unwrap()],
        dial_peers: vec!["tcp://203.0.113.1:8000".parse().unwrap()],
        ..Default::default()
    };

    // Dials `dial_peers`; see karyon_kademlia to find more peers.
    let node = Node::new(&key_pair, config, global_executor());

    node.attach_protocol(EchoProtocol::new)
        .await
        .expect("Attach echo protocol");

    node.run().await.expect("Run the node");

    node.shutdown().await;
};
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `tokio` | Use tokio async runtime (default) |
| `smol` | Use smol async runtime |
| `quic` | Enable QUIC transport |
| `serde` | Enable serde support for p2p types |

```toml
# Default (tokio + TCP/TLS)
karyon_p2p = "1.0"

# With QUIC
karyon_p2p = { version = "1.0", features = ["quic"] }

# With smol
karyon_p2p = { version = "1.0", default-features = false, features = ["smol", "quic"] }
```

## Architecture

```TEXT
                            Node
                              |
   +-----------+--------------+---------------+------------------+
   |           |              |               |                  |
Discovery  Connector /    PeerPool       ProtocolRegistry     Monitor
           Listener           |          (ctor + meta)          ^
               |              |               |                 |
               +---queue--> admission <-------+            PeerEvents
                              |                                 ^
                          Sessions <-- Broadcaster              |
                              |            ^             (pool reports
                     Peer (one task each)  |              each event once)
                              |            |
               PeerConn per protocol ------+--> Protocol
                              |
                   karyon_net (TCP / TLS / QUIC)
```

- **Node**: top-level lifecycle. Creates the discovery, registry, pool,
  connector and listener; wires them together; exposes the public API.
- **Discovery**: finds peers and yields them as `DiscoveredPeer`. The
  default finds none; karyon_kademlia and custom implementations of the
  `Discovery` trait plug in. The node dials `dial_peers` itself.
- **Connector / Listener**: dial and accept connections over the
  configured transports, then hand them to the PeerPool over a bounded
  queue. Their tasks end at the handover.
- **ProtocolRegistry**: the attached protocols (constructor and
  version), shared by the Node and the PeerPool.
- **PeerPool**: takes connections from the queue, admits them and runs
  one Peer per admitted connection.
- **Admission**: the handshake (version + protocol negotiation, peer
  identity bound to the secure transport's certificate) and access
  control, before any Peer is built.
- **Sessions**: one session per connected peer; sends and broadcasts.
  Shared with every protocol's broadcaster.
- **PeerEvents**: the pool reports each peer event once; it goes to the
  monitor, the `register_peer_events` listeners and the discovery.
- **Peer**: one task per connection. Starts the negotiated protocols and
  stops on the first error. Knows nothing about the pool.
- **PeerConn / Protocol**: each protocol gets a `PeerConn` scoped to its
  own id: `send`, `recv`, `broadcast`, and `info()` for the peer's
  `PeerInfo`. Custom protocols implement the `Protocol` trait.

### Transport

Peers can listen and connect over multiple transports simultaneously.
Configuration uses endpoint URLs (`tcp://`, `tls://`, `quic://`); TLS
uses self-signed certs derived from the node's keypair, and QUIC
multiplexes one stream per protocol.

### Discovery

Implementations of the `Discovery` trait yield candidate peers; the
Node dials them. The Node also dials `Config::dial_peers` once at start,
with any discovery. `Node::new` runs no discovery. For a Kademlia DHT use
[karyon_kademlia](../kademlia) through `Node::with_discovery`, or plug in
your own (mDNS, ...) by implementing the trait: only `start` and `recv`
are required. The closure given to `with_discovery` gets a
`DiscoveryContext`: the node's key pair, PeerID, config, monitor and
executor.

### Protocols

A built-in `Ping` keep-alive runs on every connection; everything else
is a custom protocol. For TCP/TLS, protocols share a single framed
connection. For QUIC, each protocol gets its own bidirectional stream.

`Protocol::flags()` defaults to `ProtocolFlags::PREFERRED`. Override to
`ProtocolFlags::REQUIRED` for protocols every peer must speak, or
`ProtocolFlags::empty()` for protocols that should not be advertised.
Flags combine with `|`. Bits from `ProtocolFlags::USER` up are free for
user meaning; each `Discovery` implementation decides what they do.

### Monitor

Subscribe to `node.monitor()` to observe what the network is doing -
connections opening and closing, peer add/remove, handshake failures,
discovery events. Each topic is a separate event type
with its own listener; every subscriber gets every event independently.

```rust,ignore
let listener = node.monitor().register::<ConnectionEvent>();
while let Ok(ev) = listener.recv().await {
    println!("conn event: {} {:?}", ev.event, ev.endpoint);
}
```

Available event types: `ConnectionEvent`, `PeerPoolEvent`,
`DiscoveryEvent`. Gated by `Config::enable_monitor`.

### Network Security

TLS is available for TCP connections. QUIC has built-in TLS 1.3. The
p2p layer generates self-signed certificates from the node's key pair
for mutual authentication.

## Examples

See [karyon_kademlia's examples](../kademlia/examples) for a basic peer
node and a chat application.
