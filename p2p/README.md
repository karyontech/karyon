# Karyon p2p

A lightweight, extensible, and customizable peer-to-peer network stack.
Multi-transport (TCP, TCP/TLS, QUIC) with pluggable custom protocols via the
`Protocol` trait, and pluggable discovery.

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

    // Dials `dial_peers` only; see Discovery to find more peers.
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
| `aws-lc-rs` | rustls crypto provider: aws-lc-rs (default) |
| `ring` | rustls crypto provider: ring. Use with `default-features = false` |

```toml
# Default (tokio + TCP/TLS)
karyon_p2p = "1.7"

# With QUIC
karyon_p2p = { version = "1.7", features = ["quic"] }

# With smol
karyon_p2p = { version = "1.7", default-features = false, features = ["smol", "quic", "aws-lc-rs"] }

# With the ring provider instead of aws-lc-rs
karyon_p2p = { version = "1.7", default-features = false, features = ["tokio", "quic", "ring"] }
```

## Architecture

```TEXT
Node
 |-- Discovery          finds peers to dial
 |-- Connector/Listener dial / accept --(queue)--+
 |-- ProtocolRegistry   attached protocols       |
 |-- Monitor            events                   v
 +-- PeerPool <----------------------------------+
      |-- admission     handshake + access control
      +-- Sessions      PeerID -> PeerSession
           +-- PeerSession   one connection
                +-- PeerConn -> Protocol   one per negotiated protocol
```

| Part | What it does |
|------|--------------|
| `Node` | Public entry point. Builds and wires the parts; `attach_protocol`, `run`, `send_to`, `broadcast`, `shutdown`. |
| Connector / Listener | Dial / accept TCP, TLS or QUIC connections, up to `outbound_slots` / `inbound_slots`, and hand them to the PeerPool. |
| `PeerPool` | Takes connections from the queue, admits them, runs one `PeerSession` per admitted connection and removes it when it ends. |
| Admission | The handshake (network version, protocol negotiation, the claimed `PeerID` checked against the TLS certificate, if any), then access control. |
| `Sessions` | Map from `PeerID` to its `PeerSession`, one per peer. When two nodes dial each other, both keep the connection dialed by the smaller `PeerID`. |
| `PeerSession` | One connection to a peer. Runs one task per negotiated protocol; ends when a protocol ends, the connection fails or the peer shuts down. |
| `PeerConn` | What a protocol gets: `send` / `recv` on its own protocol id, `broadcast` to every peer, `info()` (`SessionInfo`: peer id, direction, endpoint, negotiated protocols). |
| `ProtocolRegistry` | The protocols attached with `attach_protocol`: constructor, version and flags. |
| Peer events | Added / removed / handshake failed, sent to the monitor, the `register_peer_events` listeners and the discovery. |

### Transport

Peers can listen and connect over multiple transports simultaneously.
Configuration uses endpoint URLs (`tcp://`, `tls://`, `quic://`); TLS
uses self-signed certs derived from the node's keypair, and QUIC
multiplexes one stream per protocol.

To bring your own transport, implement `transport::Transport` (dial and
listen, both give a byte stream) and set it with
`ConfigBuilder::with_transport` (or the `Config::transport` field). The node
uses it for the endpoints it `supports` and the built-in transports for
the others. A transport can also claim its own scheme: `Endpoint::Custom`
(`scheme://addr`, e.g. `mem://node-a`) works in `listen_endpoints`,
`dial_peers` and Kademlia once the transport `supports` it. If its
streams carry the peer's karyon certificate
(`ByteStream::peer_certificates`), the handshake checks the peer's id
against it, as with TLS.

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
