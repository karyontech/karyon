# Changelog

## 1.1.0 karyon_p2p, karyon_swarm

### Breaking changes

- **`ProtocolKind` replaced by `ProtocolFlags`** (`karyon_p2p`):
  `Protocol::kind()` is now `Protocol::flags()` and returns bit flags.
  `Mandatory` maps to `ProtocolFlags::REQUIRED`, `Optional` to
  `ProtocolFlags::PREFERRED` (the default). Flags combine with `|`;
  `ProtocolFlags::empty()` attaches a protocol without advertising it, and
  bits from `ProtocolFlags::USER` up are free for custom discovery
  implementations. `ProtocolMeta.kind` is renamed to `flags`.
- **`Discovery::advertise`** (`karyon_p2p`): new required trait method.
  Discovery now owns what the node advertises; `Node` no longer holds a
  bloom. `Node::bloom_add_mandatory`, `Node::bloom_add_optional` and
  `Node::bloom_snapshot` are removed in favor of `Node::advertise(item,
  flags)`. `KademliaDiscovery::new` no longer takes a `BloomRef`. `Bloom` and
  `BloomRef` are no longer exported; they are internal to Kademlia.
- **Kademlia wire format** (`karyon_p2p`): `PeerMsg` now carries a single
  128-bit bloom of advertised items instead of separate mandatory/optional
  filters. The required/preferred split is local filtering policy and never
  leaves the node. Items advertised with only user flags are visible to
  `find_peers_with` but do not affect peer selection. Nodes on 1.1.0 cannot
  exchange discovery messages with 1.0.x nodes.

## 1.0.1

### Fixes

- **Shutdown/admission race** (`karyon_p2p`): `PeerPool::shutdown` could miss a
  peer admitted concurrently by the run loop, leaving its connection alive so
  the remote node never observed a disconnect. The task group is now cancelled
  before closing peers, so admission and shutdown are linearized.
- **Multi-threaded global executor** (`karyon_core`): on smol, `global_executor()`
  now runs one worker thread per core instead of a single thread, and logs task
  panics instead of swallowing them. On tokio, the redundant driver thread is gone.

All crates are released as `1.0.1` in lockstep.

## 1.0.0

First stable release. Major overhaul across all crates. The changes below
apply to the `1.0.0` release of every workspace crate.

### Breaking changes

- **License changed** from GPL-3.0 to MIT.
- **Per-crate versioning**: each crate now has its own version, bumped
  independently. 
- **Network stack rewrite** (`karyon_net`): layered transport design with
  composable middleware. TCP, TLS, WebSocket, QUIC, SOCKS5, Unix, UDP.
- **JSON-RPC transport additions** (`karyon_jsonrpc`): HTTP/1.1, HTTP/2,
  HTTP/3, QUIC in addition to TCP/TLS/WebSocket/Unix.

### New crates

- **`karyon_swarm`** - Swarm layer on top of `karyon_p2p` for protocol-aware
  peer groups. Each swarm is identified by a `SwarmKey` derived from a
  protocol ID and instance name. Peers are automatically assigned to swarms
  based on their negotiated protocol set. Includes scoped `broadcast`,
  `join`/`leave` API, and peer-per-swarm queries.

### New features

- **QUIC transport support** added to `karyon_net`, `karyon_jsonrpc`, and
  `karyon_p2p`.
- **Pluggable discovery** (`karyon_p2p`): the discovery layer is now
  abstracted behind a `Discovery` trait, allowing custom backends such as
  mDNS in addition to the built-in Kademlia DHT.
- **SOCKS5 proxy support** (`karyon_net`).
