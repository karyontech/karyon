# Karyon kademlia

Kademlia DHT peer discovery for [karyon_p2p](https://crates.io/crates/karyon_p2p) nodes in Rust.

It keeps a routing table of known peers, fills it with lookups starting from bootstrap peers,
refreshes entries with liveness pings, and hands the node peers to dial. Lookups run over TCP or
QUIC and refresh pings over UDP, or all of it over a custom transport.

Use cases:

- Find peers in a decentralized network from a few bootstrap (seed) nodes
- Dial only peers that advertise the protocols or karyon_swarm swarms the node needs
- Run discovery over your own transport, including on WASI

## Install

```bash
$ cargo add karyon_p2p karyon_kademlia
```

## Example

Pass `KademliaDiscovery` to `Node::with_discovery`. The closure receives a `DiscoveryContext` with
the node's key pair, `PeerID`, config, monitor, and executor.

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

## Configuration

`KademliaConfig::discovery_endpoints` takes two sockets: a stream endpoint for the lookup service
(`tcp://`, or `quic://` with the `quic` feature) and a `udp://` endpoint for the refresh service.
Leave it empty to only look up the bootstrap peers.

`public_ip`, if set (e.g. learned with karyon_stun), replaces the IP of every advertised listen and
discovery endpoint of the same family. Ports are kept, so they must be reachable from outside.

The other fields tune the seeding interval, lookup slots, timeouts and retries, and the refresh
interval. Node settings such as `listen_endpoints`, `version`, `access_control`, and
`handshake_timeout` come from the node's `Config`.

## Transport

Lookups dial and listen through the node's `Config::transport` for the endpoints it supports,
including custom schemes such as `mem://node-a`, and use the built-in TCP or QUIC for the others.

Without a transport, refresh pings entries over UDP. Entries with no UDP address are pinged over the
lookup protocol. With a transport, the node sends no raw UDP. Every refresh ping goes over the
lookup protocol and the `udp://` discovery endpoint is not used or advertised.

## Advertised items

Attached protocols are advertised with their `Protocol::flags()`. Other layers add items with
`Node::advertise`, e.g. karyon_swarm advertises its swarm keys. Items go into a bloom filter sent to
other peers, and filter which routing table peers the node dials.

A peer must advertise every `REQUIRED` item and, if any `PREFERRED` item is set, at least one of
them. Items with only `ProtocolFlags::USER` bits are advertised but do not affect peer selection.
Items with empty flags are not advertised.

## Monitor

Kademlia posts `DiscoveryEvent`s to the node's monitor
(`node.monitor().register::<DiscoveryEvent>()`): `LookupStarted`, `LookupFailed`,
`LookupSucceeded` (with the number of peers found), `RefreshStarted`, `RefreshSucceeded` (with the
number of entries pinged), `RefreshFailed`, and `EntryEvicted`.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `tokio` | Use tokio async runtime (default) |
| `smol` | Use smol async runtime |
| `quic` | Lookups over QUIC too (default) |
| `aws-lc-rs` | Default rustls crypto provider: aws-lc-rs (default) |
| `ring` | Default rustls crypto provider: ring. Use with `default-features = false` |

The provider features are passed to karyon_p2p. `quic` needs one of them.

## WASI

karyon_kademlia builds for `wasm32-wasip1` and `wasm32-wasip2` with only the `tokio` feature. `quic`
does not build for wasm.

There is no UDP and no built-in transport on wasm. Lookups dial and listen only through the node's
`Config::transport`, and refresh always pings over the lookup protocol. See karyon_p2p's README for
the runtime and transport.

```toml
karyon_kademlia = { version = "0.4", default-features = false, features = ["tokio"] }
```

## Examples

See [examples/](./examples) for a basic peer node and a chat application.
