# Karyon Swarm

A swarm layer on top of [karyon_p2p](https://crates.io/crates/karyon_p2p)
for protocol-aware peer groups, with BitTorrent-style swarm-keyed
discovery.

Each swarm is identified by a `SwarmKey` derived from a protocol id and
an instance name (e.g. `("ChatProto", "general")`). Peers with different
protocol sets can coexist; peers are automatically assigned to the
swarms they support based on their negotiated protocols.

## Install

```bash
$ cargo add karyon_swarm
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `tokio` | Use tokio async runtime (default) |
| `smol` | Use smol async runtime |
| `aws-lc-rs` | rustls crypto provider: aws-lc-rs (default) |
| `ring` | rustls crypto provider: ring. Use with `default-features = false` |

```toml
karyon_swarm = "2.0"
```

### WASI

karyon_swarm builds for `wasm32-wasip1` and `wasm32-wasip2` with only
the `tokio` feature, on a karyon_p2p node with your own runtime
and transport (see karyon_p2p's README).

```toml
karyon_swarm = { version = "2.0", default-features = false, features = ["tokio"] }
```

## Example

```rust,no_run
use karyon_core::async_runtime::global_executor;
use karyon_p2p::{Node, Config, keypair::{KeyPair, KeyPairType}};
use karyon_swarm::Swarm;

async {
    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);

    // `Node::new` only dials `Config::dial_peers`; use karyon_kademlia
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

    // Send a message to all peers in the "general" chat swarm.
    // swarm.broadcast(&chat_key, msg_bytes).await;

    // Connected peers currently in this swarm.
    // let peers = swarm.peers(&chat_key).await;

    swarm.shutdown().await;
};
```

## Limitations

**Late `join` does not actively reshape the connection pool.** The
connector fills outbound slots at startup based on the local node's
state at that moment. If you call `swarm.join(...)` *after*
`swarm.run()`, the slots are already occupied by peers picked under the
previous state; the new swarm only gains members as those existing
connections churn (timeout, ping fail, explicit shutdown). On a stable
network this can leave a freshly-joined swarm empty for a long time.

Recommended pattern today: register every swarm via `swarm.join(...)`
*before* calling `swarm.run()`.

A future release will add proactive dialing for late joins (and
optionally eviction of non-member peers to make room) so swarm
membership can be reshaped at runtime without restarting the node.
