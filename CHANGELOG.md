# Changelog

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
