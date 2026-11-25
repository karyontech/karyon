# Karyon p2p

A lightweight, extensible, and customizable peer-to-peer (p2p) network stack for Rust.

A `karyon_p2p` node listens on and dials peers over TCP, TLS, or QUIC, identifies each peer by a
`PeerID` derived from its key pair, and runs the protocols you attach over each connection.
Transports, async runtimes, protocols, and peer discovery are pluggable. Bring your own transport
and runtime to run karyon in custom environments, including WASI and executors other than
tokio and smol.

Use cases:

- Decentralized applications and overlay networks in Rust
- Custom message protocols between peers over TCP, TLS 1.3, or QUIC
- Kademlia DHT peer discovery with [karyon_kademlia](../kademlia)
- Nodes on WASI or a custom executor, using a transport you supply

## Install

```bash
$ cargo add karyon_p2p
```

## Example

An echo node: it attaches a custom protocol that broadcasts every message
it receives to all connected peers, the sender included.

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

    // Dials `dial_peers` only. See Discovery to find more peers.
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
| `smol` | Use smol async runtime. Enable exactly one of `tokio` and `smol` |
| `quic` | Enable the built-in QUIC transport (default). Needs `aws-lc-rs` or `ring`, not on wasm |
| `serde` | `Serialize` / `Deserialize` for the monitor event types |
| `aws-lc-rs` | Default rustls crypto provider: aws-lc-rs (default). Wins if `ring` is also on |
| `ring` | Default rustls crypto provider: ring. Use with `default-features = false` |

```toml
# Default (tokio, TCP/TLS/QUIC, aws-lc-rs)
karyon_p2p = "2.1"

# Without QUIC
karyon_p2p = { version = "2.1", default-features = false, features = ["tokio", "aws-lc-rs"] }

# With smol
karyon_p2p = { version = "2.1", default-features = false, features = ["smol", "quic", "aws-lc-rs"] }

# With the ring provider instead of aws-lc-rs
karyon_p2p = { version = "2.1", default-features = false, features = ["tokio", "quic", "ring"] }
```

TLS and QUIC use a configurable rustls `CryptoProvider` (`Config::crypto_provider`), defaulting
to aws-lc-rs, ring, or the process default. QUIC requires a provider feature, TLS does not.

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
| `Node` | Public entry point. Builds and wires the parts. Dials `Config::dial_peers` once, then the peers the discovery yields, skipping denied endpoints and peers already connected. `attach_protocol`, `run`, `send_to`, `broadcast`, `broadcast_to`, `advertise`, `register_peer_events`, `monitor`, `shutdown`. |
| `Connector` / `Listener` | Dial / accept through `Config::transport` for the endpoints it `supports`, else through the built-in TCP, TLS or QUIC (only the user transport on wasm). Each connection holds an `outbound_slots` / `inbound_slots` slot until it closes and is queued to the `PeerPool`. A dial retries up to `max_connect_retries` with backoff, each try bounded by `handshake_timeout`. Also public, built from a `DiscoveryContext`, for a discovery's own connections. |
| `AccessControl` | `Config::access_control`, `AllowAll` by default. Checks the endpoint before a dial and after an accept, and the peer after the handshake. |
| `PeerPool` | Takes connections from the queue, admits each in its own task, runs one `PeerSession` per admitted connection and removes it when it ends. |
| Admission | The handshake (network version, protocol negotiation with every `REQUIRED` protocol present, the claimed `PeerID` checked against the peer certificate of TLS, QUIC or a user transport, if any), then `AccessControl`. |
| `Sessions` | Map from `PeerID` to its `PeerSession`, one per peer. When two nodes dial each other, both keep the connection dialed by the smaller `PeerID`. For two connections in the same direction, both keep the one with the smaller connection key from the handshake. |
| `PeerSession` | One connection to a peer. Opens the QUIC protocol streams, then runs one task per negotiated protocol. Ends when a protocol returns, the connection fails, the peer shuts down or another connection to the same peer replaces it. |
| `PeerConn` | What a protocol gets: `send` / `recv` on its own protocol id, `broadcast` to every peer, `info()` (`SessionInfo`: peer id, direction, endpoint, negotiated protocols). |
| `ProtocolRegistry` | The protocols attached with `attach_protocol`: constructor, version and flags. |
| Peer events | Added / removed / handshake failed, sent to the monitor, the `register_peer_events` listeners (`PeerEvent`) and the discovery (`PeerConnectionEvent`, which also reports failed dials). |
| `Discovery` | Yields candidate peers to the `Node`. `Node::new` runs none. |
| `Monitor` | Emits connection, peer pool and discovery events when `Config::enable_monitor` is on. |

### Transport

karyon supports TCP, TCP/TLS, and QUIC out of the box. A node can listen and connect over multiple
transports simultaneously using endpoint URLs (`tcp://`, `tls://`, `quic://`).

TLS uses self-signed certificates derived from the node's key pair. QUIC uses a separate
bidirectional stream for each protocol.

**Custom transports**

To use your own transport, implement `karyon_net::transport::Transport` (re-exported as
`karyon_p2p::transport`) and register it through `ConfigBuilder::with_transport` or
`Config::transport`.

The trait provides `supports`, `dial` and `listen`. `dial` and `listen` produce byte streams. The
node uses your transport for the endpoints it `supports`, built-in schemes included, and falls
back to the built-in transports for the others. Its streams are used as is: no TLS is added, and
all protocols share one framed connection, even on a `quic://` endpoint.

Custom transports can also introduce new endpoint schemes through `Endpoint::Custom`, such as
`mem://node-a`. These endpoints work with `listen_endpoints`, `dial_peers`, and Kademlia discovery
once the transport `supports` them.

If the transport exposes peer certificates through `ByteStream::peer_certificates`, the handshake
verifies the peer's identity against its certificate, just as with TLS.

### Runtime

karyon supports both **tokio** (default) and **smol**, selected through Cargo features.

You can also bring your own async runtime by implementing `karyon_core::async_runtime::Runtime`,
which provides task scheduling (`schedule`) and timers (`sleep`).

Pass your runtime to the node using `Executor::from(runtime)`. All node tasks and timers then run
on your executor.

Combined with a custom `Transport`, this allows karyon to run in environments without a built-in
networking stack or standard async runtime.

The built-in QUIC runs its tasks on the node's executor. To run it on your own I/O reactor (tasks,
timers and UDP sockets), set `Config::quic_runtime` (or `ConfigBuilder::with_quic_runtime`).

See [examples/custom_runtime.rs](examples/custom_runtime.rs) for two nodes communicating over
in-memory pipes on a single-threaded runtime.

### WASI

karyon_p2p builds for `wasm32-wasip2` and `wasm32-wasip1` with only the `tokio` feature. No crypto
provider or C toolchain is required. `quic` does not build on wasm.

Built-in TCP, TLS, and QUIC transports are unavailable on WASI. The node dials and listens only
through `Config::transport`. Provide your own `Runtime` and `Transport` as described above.

**Dependency:**

```toml
karyon_p2p = { version = "2.1", default-features = false, features = ["tokio"] }
```

**Run the custom runtime example with wasmtime:**

```bash
CARGO_TARGET_WASM32_WASIP2_RUNNER=wasmtime cargo run -p karyon_p2p \
    --example custom_runtime --target wasm32-wasip2 \
    --no-default-features --features tokio
```

### Discovery

Peer discovery is pluggable. Implementations of the `Discovery` trait find candidate peers, which
the node then attempts to connect to.

The node dials the peers in `Config::dial_peers` once at startup, with or without discovery.
`Node::new` starts without discovery, so it dials only those.

To enable discovery, use `Node::with_discovery` with either:

- **Kademlia DHT:** Use the [karyon_kademlia](../kademlia) crate for decentralized peer discovery.
- **Custom discovery:** Implement the `Discovery` trait for other mechanisms, such as mDNS.

A custom discovery implementation only needs to implement `start` and `recv`. The optional
`on_event` receives `PeerConnectionEvent`s from the node, and `advertise` receives the items passed
to `Node::advertise`, including the id and flags of every attached protocol.

The factory closure passed to `Node::with_discovery` receives a `DiscoveryContext`, providing
access to the node's key pair, `PeerID`, configuration, monitor, and executor.

### Protocols

A built-in `Ping` keep-alive runs on every connection. Other protocols are pluggable. `Ping` is
`REQUIRED` and uses `Config::ping_interval` and `Config::ping_timeout`. On TCP, TLS and a user
transport, protocols share one framed connection. The built-in QUIC uses a separate bidirectional
stream per protocol.

`Protocol::flags()` defaults to `ProtocolFlags::PREFERRED`. Use `ProtocolFlags::REQUIRED` to
reject peers that do not speak the protocol, or `ProtocolFlags::empty()` to negotiate it without
advertising it. Flags can be combined with `|`, and bits from `ProtocolFlags::USER` onward are
free for user meaning. Each `Discovery` implementation decides what they do.

### Monitor

Subscribe to `node.monitor()` to observe what the network is doing: connections opening and
closing, peer add/remove, handshake failures and discovery events. Each topic is a separate event
type with its own listener. Every subscriber gets every event independently.

```rust,ignore
let listener = node.monitor().register::<ConnectionEvent>();
while let Ok(ev) = listener.recv().await {
    println!("conn event: {} {:?}", ev.event, ev.endpoint);
}
```

Available event types: `ConnectionEvent`, `PeerPoolEvent`, `DiscoveryEvent`. Events are emitted
only when `Config::enable_monitor` is `true` (default `false`).

### Network Security

`tcp://` is plain TCP, with no encryption or authentication. `tls://` and `quic://` use TLS 1.3
only, with mutual authentication. Each node presents a self-signed certificate whose extension
holds its `PeerID`, signed by the node's key pair. A dial to a known `PeerID` fails if the
certificate does not match it, and the handshake rejects a peer whose claimed `PeerID` differs
from its certificate.

## Examples

- [karyon_kademlia examples](../kademlia/examples): a basic peer node and a chat application.
- [examples/custom_runtime.rs](examples/custom_runtime.rs): nodes on a runtime and transport you
  supply, also on WASI.
