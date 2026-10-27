# Changelog

## karyon_core 1.0.4, karyon_net 1.0.4, karyon_jsonrpc 1.0.6, karyon_p2p 1.1.6, karyon_swarm 1.1.6, karyon_stun 0.1.3

### Breaking changes

- **WebSocket message limit** (`karyon_net`, `karyon_jsonrpc`): WS messages
  and frames over 4 MiB are now rejected by default. Before, tungstenite's
  64 MiB limit applied and the codec limit was checked only after the whole
  message was buffered. Use `WsLayer::with_max_msg_size` to allow more.
- **New `QuicConfig` fields** (`karyon_net`): `stream_receive_window` and
  `receive_window`. Code that builds `QuicConfig` with a full struct literal
  must set them, or use `..Default::default()`.

### Added

- **`lookup_host`** (`karyon_core::async_runtime::net`): resolves a host
  name on a blocking thread pool, for both smol and tokio.
- **`WsLayer::with_max_msg_size`** (`karyon_net`): max size of one WS
  message or frame.
- **`TlsListener::with_handshake_timeout`** and
  **`ServerQuicConfig::with_handshake_timeout`** (`karyon_net`): handshake
  timeout of the `accept` helpers, 10s by default.

### Fixes

- **DNS on the executor** (`karyon_net`, `karyon_p2p`): `tcp::connect`,
  `udp::dial` and `QuicEndpoint::dial` resolve domains off the executor,
  under a 10s timeout. Before, a slow DNS server stalled executor threads.
- **QUIC receive windows** (`karyon_net`): a connection buffers at most
  8 MB of unread data by default, 1 MB per stream. Before, the connection
  window was unbounded, so one peer could pin ~250 MB.
- **SOCKS5 host length** (`karyon_net`): `Socks5Layer` returns an error for
  empty or over 255 byte hosts. Before, a long host wrapped its one byte
  length and the proxy read a different host and port.
- **Accept handshake timeout** (`karyon_net`): `TlsListener::accept` and
  `QuicEndpoint::accept` now fail if the handshake takes longer than the
  handshake timeout, so a silent client no longer blocks an accept loop.

## karyon_core 1.0.3, karyon_net 1.0.3, karyon_jsonrpc 1.0.5, karyon_p2p 1.1.5, karyon_swarm 1.1.5, karyon_stun 0.1.2, karyon_eventemitter 1.0.2

### Breaking changes

- **`Error::Rand`** (`karyon_p2p`): now wraps `rand::rngs::SysError` from
  rand 0.10, instead of `rand::rand_core::OsError`.

### Fixes

- **Weak Ed25519 keys** (`karyon_core`, `karyon_p2p`): `PublicKey::from_bytes`
  now rejects small-order keys, and `verify` uses strict verification.
  Before, a weak key with a crafted signature verified for any message, so
  anyone could claim that key's PeerID in the p2p TLS handshake.
- **`CondVar`** (`karyon_core`): no longer panics with "All IDs exhausted"
  when many tasks wait on it, and a `signal()` is no longer lost when the
  woken waiter is cancelled before it runs.
- **`TaskGroup` after a panic** (`karyon_core`, `karyon_p2p`,
  `karyon_jsonrpc`): `cancel()` no longer hangs after a task or its callback
  panics, and panicked tasks are removed from the group. A panic in a p2p
  protocol handler or a jsonrpc request task no longer hangs `Node`, `Peer`
  or `Server` shutdown.

## 1.1.4 karyon_p2p, karyon_swarm

### Breaking changes

- **Network version 0.2.0** (`karyon_p2p`): `Config::default().version` is
  now `0.2.0`, and the handshake wire format changed. 

### Fixes

- **Protocol negotiation** (`karyon_p2p`): both peers now send a version
  message and compute the shared protocol set with the same rule, so both
  sides run the same protocols. Before, the outbound side ran all of its own
  protocols, even ones the remote did not support.

## 1.0.4 karyon_jsonrpc

### Fixes

- **Custom codec over HTTP** (`karyon_jsonrpc`): HTTP/1.1, HTTP/2 and HTTP/3
  clients and servers now use the codec set on the builder. Before, they
  always used the default JSON codec and ignored custom codecs and
  `JsonCodec::new(max_size)`.
- **HTTP/3 pub/sub** (`karyon_jsonrpc`): the client now reads notifications
  one codec frame at a time, so large or back-to-back notifications on a
  subscription stream are decoded correctly.

## 1.1.3 karyon_p2p, karyon_swarm

### Fixes

- **QUIC transport** (`karyon_p2p`): nodes can now listen on and dial
  `quic://` endpoints. Before, `Node::run` failed with
  `no initial cipher suite found`, because QUIC needs TLS13_AES_128_GCM_SHA256
  for its initial packets. Plain TLS still offers only ChaCha20-Poly1305.
- **Lost first message on QUIC streams** (`karyon_p2p`): the protocol header
  at the start of each QUIC stream is now length-prefixed, so a message sent
  right after it is no longer dropped. This changes the QUIC wire format.

## 1.0.2 karyon_core

### Added

- **`Semaphore`** (`karyon_core::async_runtime::lock`): async counting
  semaphore for both smol and tokio. `acquire()` returns a `SemaphoreGuard`
  that releases the permit on drop and can be moved into other tasks.
- **`Error::TokioAcquireError`** (`karyon_core`, tokio only): returned by
  `Semaphore::acquire` if the underlying tokio semaphore is closed.

## 1.1.2 karyon_p2p, karyon_swarm

### Fixes

- **Connection slot limits** (`karyon_p2p`): `inbound_slots`,
  `outbound_slots`, `lookup_inbound_slots` and `lookup_outbound_slots` are
  now strictly enforced. Concurrent dials or accepts could exceed them before.

## 1.0.2 karyon_jsonrpc

### Added

- **`Channel::peer_endpoint()`** (`karyon_jsonrpc`): returns the remote
  endpoint of the connection, or `None` when the transport does not report one.

## karyon_stun 0.1.0

- New crate: minimal async STUN (RFC 5389) server and client over UDP,
  TCP and TLS, built on karyon_net. Binding requests only; supports
  XOR-MAPPED-ADDRESS, MAPPED-ADDRESS, ERROR-CODE, and SOFTWARE.
- Optional QUIC transport (`quic` feature). Not part of RFC 5389:
  a karyon extension that only works between karyon nodes.

## 1.1.1 karyon_p2p, karyon_swarm

### Breaking changes

- **`Config::public_ip`** (`karyon_p2p`): when set, discovery advertises
  this IP in place of the IP of each listen and discovery endpoint of the
  same family, keeping the configured ports. Use `karyon_stun` (or any
  other source) to learn it. `None` by default (no change). Code building
  `Config` with a struct literal and no `..Default::default()` must add the
  field.

### Fixes

- **Unreachable peers in routing tables** (`karyon_p2p`): a node with no
  listen and no discovery endpoints no longer sends a `PeerMsg` after a
  lookup, and received `PeerMsg`s with no addresses are ignored instead of
  added to the routing table.

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
