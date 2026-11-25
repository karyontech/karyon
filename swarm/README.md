# Karyon Swarm

Protocol-aware peer groups (swarms) for [karyon_p2p](https://crates.io/crates/karyon_p2p) in Rust,
with BitTorrent-style swarm-keyed peer discovery.

Each swarm is identified by a `SwarmKey`, a hash of a protocol ID and an optional instance name.
Peers with different protocol sets can coexist, and each connected peer is added to every swarm
whose protocol it negotiated.

Use cases:

- Several p2p applications or protocols on one node, each with its own peer group
- Rooms, topics, or channels inside one protocol
- Broadcast to the members of one group, and find them with karyon_kademlia

## Install

```bash
$ cargo add karyon_p2p karyon_swarm
```

## Example

```rust,no_run
use karyon_core::async_runtime::global_executor;
use karyon_p2p::{Node, Config, keypair::{KeyPair, KeyPairType}};
use karyon_swarm::Swarm;

async {
    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);

    // `Node::new` only dials `Config::dial_peers`. Use karyon_kademlia
    // (`Node::with_discovery`) to find peers that share a swarm.
    let node = Node::new(&key_pair, Config::default(), global_executor());
    let swarm = Swarm::new(node, global_executor());

    // Join a swarm. SwarmKey defaults to hash(ProtocolID); call multiple
    // times with different protocols (or instances) to manage many swarms
    // from the same Swarm instance.
    // let chat_key = swarm.join(ChatProtocol::new).await?;
    // For sub-grouping inside a protocol (rooms, topics):
    // let room_key = swarm.join_with_instance("general", ChatProtocol::new).await?;

    swarm.run().await.expect("run swarm");

    // Send a message to all peers in the chat swarm.
    // swarm.broadcast(&chat_key, msg_bytes).await;

    // Connected peers currently in this swarm.
    // let peers = swarm.peers(&chat_key).await;

    swarm.shutdown().await;
};
```

`Swarm::new(node, ex)` wraps a node. Its membership task runs on `ex`. `run` starts the node and
tracks peer events. `shutdown` stops both.

## Swarms

`join` attaches a protocol and returns its key, `sha256(protocol_id)`. `join_with_instance` adds an
instance name for sub-groups inside one protocol, such as chat rooms or topics:
`sha256(protocol_id + ":" + instance)`. `swarm_key_from_protocol` and `compute_swarm_key` compute
the same keys.

Joining also advertises the key as `ProtocolFlags::PREFERRED` through `Node::advertise`, so a
discovery such as karyon_kademlia picks peers that share a swarm. Peers already connected that speak
the protocol are added right away.

`broadcast` sends to the connected peers of one swarm. `peers` and `peers_len` return its members.
`leave` drops local membership only. The protocol stays attached and the key stays advertised.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `tokio` | Use tokio async runtime (default) |
| `smol` | Use smol async runtime |
| `aws-lc-rs` | Default rustls crypto provider: aws-lc-rs (default) |
| `ring` | Default rustls crypto provider: ring. Use with `default-features = false` |

The provider features are passed to karyon_p2p.

```toml
karyon_swarm = "2.0"
```

## WASI

karyon_swarm builds for `wasm32-wasip1` and `wasm32-wasip2` with only the `tokio` feature, on a
karyon_p2p node with your own runtime and transport (see karyon_p2p's README).

```toml
karyon_swarm = { version = "2.0", default-features = false, features = ["tokio"] }
```

## Limitations

A late `join` does not reshape the connection pool. The connector fills outbound slots based on the
node's state at that time. If you call `join` after `run`, the slots are already taken by peers
picked earlier, and the new swarm only gains members as those connections churn (timeout, failed
ping, shutdown). On a stable network a late swarm can stay empty for a long time.

Call `join` for every swarm before `run`. Proactive dialing for late joins, and optionally evicting
non-member peers, is planned for a future release.
