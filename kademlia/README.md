# Karyon kademlia

Kademlia DHT discovery for [karyon_p2p](https://crates.io/crates/karyon_p2p).
It keeps a routing table of known peers, fills it with lookups through
bootstrap peers, checks entries with UDP liveness pings, and hands the
node peers to dial.

## Install

```bash
$ cargo add karyon_p2p karyon_kademlia
```

## Example

```rust,no_run
use karyon_core::async_runtime::global_executor;
use karyon_kademlia::{KademliaConfig, KademliaDiscovery};
use karyon_p2p::{
    Config, Node,
    keypair::{KeyPair, KeyPairType},
};

async {
    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);

    // Node settings: where to listen, slots, timeouts, ...
    let config = Config {
        listen_endpoints: vec!["tcp://0.0.0.0:8000".parse().unwrap()],
        ..Config::default()
    };

    // Kademlia settings: its own sockets and the seeds.
    let kconfig = KademliaConfig {
        discovery_endpoints: vec![
            "tcp://0.0.0.0:7000".parse().unwrap(),
            "udp://0.0.0.0:7000".parse().unwrap(),
        ],
        bootstrap_peers: vec!["tcp://seed.example.com:7000".parse().unwrap()],
        ..KademliaConfig::default()
    };

    // The closure gets the node's key pair, PeerID, config, monitor and
    // executor.
    let node = Node::with_discovery(
        &key_pair,
        config,
        |ctx| KademliaDiscovery::new(ctx, kconfig),
        global_executor(),
    );

    node.run().await.expect("Run the node");
    node.shutdown().await;
};
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `tokio` | Use tokio async runtime (default) |
| `smol` | Use smol async runtime |
| `quic` | Lookups over QUIC too (default) |

## Examples

See [examples/](./examples) for a basic peer node and a chat application.
