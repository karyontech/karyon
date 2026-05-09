# Karyon Swarm

A swarm layer on top of [karyon_p2p](https://crates.io/crates/karyon_p2p)
for protocol-aware peer groups, with BitTorrent-style swarm-keyed
discovery.

Each swarm is identified by a `SwarmKey` derived from a protocol id and
an instance name (e.g. `("ChatProto", "general")`). Peers with different
protocol sets can coexist; peers are automatically assigned to the
swarms they support based on their negotiated protocols.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `smol` | Use smol async runtime (default) |
| `tokio` | Use tokio async runtime |

```toml
karyon_swarm = "1"
```

## Example

```rust
use std::sync::Arc;

use karyon_p2p::{Node, Config, keypair::{KeyPair, KeyPairType}};
use karyon_swarm::Swarm;

async {
    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
    let ex = Arc::new(smol::Executor::new());

    let node = Node::new(&key_pair, Config::default(), ex.clone().into());
    let swarm = Swarm::new(node, ex.clone().into());

    // Join a swarm. SwarmKey defaults to hash(ProtocolID); call multiple
    // times with different protocols (or instances) to manage many swarms
    // from the same Swarm instance.
    // let chat_key = swarm.join::<ChatProtocol>(ChatProtocol::new).await?;
    // For sub-grouping inside a protocol (rooms, topics):
    // let room_key = swarm.join_with_instance::<ChatProtocol>("general", ChatProtocol::new).await?;

    swarm.run().await.expect("run swarm");

    // Send a message to all peers in the "general" chat swarm.
    // swarm.broadcast(&chat_key, msg_bytes).await;

    // Connected peers currently in this swarm.
    // let peers = swarm.peers(&chat_key).await;

    // Routing-table candidates that may belong to this swarm.
    // let candidates = swarm.find_peers(&chat_key);

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
