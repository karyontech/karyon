# Changelog

## karyon_net 2.2.0

### Added

- **WASI build** (`karyon_net`): with the `tokio` and `ring` features it
  builds for `wasm32-wasip1`/`wasip2`. There are no OS sockets there: the
  TCP, UDP, Unix and QUIC transports and `TlsListener` are left out;
  framing, codecs, `Endpoint`, and the TLS and SOCKS5 layers remain.
  The `ws` feature does not build on wasm.
- **`transport::Transport`** (`karyon_net`): one trait for a user
  supplied way to dial and listen (`supports`, `dial`, `listen`, with
  `TransportListener` and `TransportRef`), for crates on karyon_net to
  take instead of their built-in transports, e.g. a custom `mem://`
  endpoint, sockets from another runtime, or WASI.
- **`examples/custom_transport.rs`** (`karyon_net`): a `mem://`
  transport over in-memory pipes with framed messages; runs natively and
  under wasmtime.

## karyon_core 2.0.0, karyon_net 2.1.0, karyon_jsonrpc 1.3.0, karyon_stun 0.4.0, karyon_p2p 1.7.0, karyon_kademlia 0.3.0, karyon_swarm 1.5.0

### Breaking changes

- **`Executor::handle()` returns `Option<&Handle>`** (`karyon_core`,
  tokio): `None` on a user runtime.
- **karyon_core 2.0** (net, jsonrpc, stun, p2p, kademlia, swarm): these
  crates now build on karyon_core 2.0; code that also uses karyon_core
  directly must move to 2.0 too.
- **tokio features trimmed** (`karyon_core`): karyon_core enables only
  the tokio features it uses (`sync`, `io-util`, `rt`, `rt-multi-thread`,
  `time`, `net`) instead of `full`. An app that used other tokio features
  (e.g. `macros`, `fs`, `signal`) through karyon must enable them on its
  own tokio dependency.

### Added

- **Bring your own runtime** (`karyon_core`): implement the `Runtime`
  trait (`schedule` a task, `sleep`) and pass it as
  `Executor::from(runtime)`. `Executor::spawn`,
  `Executor::sleep`, `Executor::timeout`, `Backoff::with_executor` and
  `TaskGroup::with_executor` then run on it. `TaskGroup::executor()`
  returns the executor of a group. The free `spawn`, `sleep` and
  `timeout` stay on smol or tokio. Locks and IO still come from the
  `smol` or `tokio` feature.
- **WASI build** (`karyon_core`): with the `tokio` feature it builds for
  `wasm32-wasip1`/`wasip2`, without the `net` module; tasks and timers
  come from a user runtime.
- **`Executor::from(Handle)`** (`karyon_core`, tokio): share the tokio
  runtime the app already runs, e.g. `Handle::current()` under
  `#[tokio::main]`, so karyon, hyper and the rest of the app use one
  runtime.
- **`TaskID`** is exported from `async_util` (`karyon_core`).

## karyon_p2p 1.6.1

### Breaking changes

- **Network version 0.4.0**: the handshake `VerMsg` carries a random
  per-connection nonce, so `Config::default().version` is now 0.4.0 and
  nodes on 0.3.x can not talk to it.

### Fixed

- **Two connections in the same direction no longer drop the peer**: a
  node that dials the same peer twice (e.g. on its TCP and TLS
  endpoints) could keep one connection while the peer kept the other,
  so both closed and the peer was lost. Both nodes now keep the
  connection with the smaller key derived from the handshake nonces.

## karyon_net 2.0.0, karyon_jsonrpc 1.2.0, karyon_stun 0.3.0, karyon_p2p 1.6.0, karyon_kademlia 0.2.0, karyon_swarm 1.4.0

### Breaking changes

- **`Endpoint::Custom` and `#[non_exhaustive]`** (`karyon_net`): an
  endpoint with a scheme karyon has no built-in transport for, e.g.
  `mem://node-a`, now parses as `Endpoint::Custom { scheme, addr }`
  instead of failing with `UnsupportedEndpoint`. `Endpoint` is
  `#[non_exhaustive]`: a `match` on it needs a `_` arm. `port()`,
  `addr()` and `SocketAddr::try_from` fail on a custom endpoint.
  jsonrpc, stun, p2p, kademlia and swarm move to net 2.0; their APIs use
  `Endpoint`, so code matching on it needs the `_` arm there too.
- **`PeerAddr` is an enum** (`karyon_p2p`): `PeerAddr::Ip { addr, port,
  protocol, priority }` for built-in transports and `PeerAddr::Custom {
  scheme, addr, priority }` for custom endpoints. Use the `priority()`,
  `protocol()` and `ip_addr()` accessors instead of the old fields.
  `PeerAddr` and `Protocol` are `#[non_exhaustive]`.
- **`pick_endpoint` takes the transport** (`karyon_p2p`):
  `pick_endpoint(addrs, supported, transport)` also picks the custom
  addresses `transport` supports; pass `None` for built-in only.
- **Network version 0.3.0** (`karyon_p2p`): `PeerAddr` changed on the
  wire, so `Config::default().version` is now 0.3.0 and nodes on 0.2.x
  can not talk to it.

### Added

- **`Endpoint::is_custom`** (`karyon_net`).

### Changed

- **Custom endpoints over the transport** (`karyon_p2p`,
  `karyon_kademlia`): with a `Config::transport` that supports them, a
  node listens on, dials and advertises custom endpoints
  (`scheme://addr`), and Kademlia looks up peers through them.
- **Custom address length** (`karyon_kademlia`): a `PeerMsg` with a
  custom scheme or address longer than 255 bytes is rejected.

## karyon_p2p 1.5.0, karyon_kademlia 0.1.3, karyon_swarm 1.3.3

### Added

- **Pluggable transport** (`karyon_p2p`): implement the new
  `transport::Transport` trait (dial and listen, returning byte streams)
  and set `Config::transport`. The node uses it for the endpoints it
  supports and the built-in TCP, TLS and QUIC for the others.
  `Config` gets the `transport` field (default `None`); a struct literal
  without `..Config::default()` must set it.
- **`ConfigBuilder`** (`karyon_p2p`): builds a `Config` from the defaults
  with `with_*` methods (`with_listen_endpoints`, `with_dial_peers`,
  `with_transport`, ...). `Config` fields stay public.
- **`Connector::new_with_transport`, `Listener::new_with_transport`**
  (`karyon_p2p`): the discovery-side Connector and Listener can use the
  node's transport.

### Changed

- **Kademlia over the transport** (`karyon_kademlia`): with
  `Config::transport` set, lookups dial and listen through it, and the
  refresh pings each entry over the lookup protocol instead of UDP; the
  `udp://` discovery endpoint is then not used or advertised.
- **Refresh without a UDP address** (`karyon_kademlia`): an entry that
  advertises no UDP address is pinged over the lookup protocol instead
  of failing its refresh and being evicted.
- **swarm** (`karyon_swarm`): requires karyon_p2p 1.5.0.

## karyon_jsonrpc 1.1.7

### Fixed

- **HTTP/3 calls failing with `H3 finish: Remote reset`** (`karyon_jsonrpc`):
  when the server answered and stopped the request stream before the
  client finished sending, the client returned that error instead of the
  response. It now reads the response.

## karyon_p2p 1.4.1, karyon_jsonrpc 1.1.6, karyon_stun 0.2.4, karyon_kademlia 0.1.2, karyon_swarm 1.3.2

### Added

- **Selectable rustls crypto provider** (`karyon_p2p`, `karyon_jsonrpc`,
  `karyon_stun`, `karyon_kademlia`, `karyon_swarm`): new features
  `aws-lc-rs` (default) and `ring`. To build with ring only, use
  `default-features = false` and enable `ring` (plus a runtime). With
  `--no-default-features`, enable one of them: p2p, kademlia and swarm
  always need one, jsonrpc and stun need one for `quic`.

### Changed

- **karyon_p2p no longer builds ring by default**: `rcgen` now uses the
  selected provider instead of its default ring backend.

## karyon_p2p 1.4.0

### Breaking changes

- **`Peer` is no longer public** (`karyon_p2p`): protocols only see their
  `PeerConn`. `PeerConn::inner()` is removed; use `PeerConn::info` instead. To
  send on another protocol, use `Node::send_to` or `Node::broadcast`.
- **`PeerPool` and `PeerPoolRef` are no longer public** (`karyon_p2p`):
  attach protocols with `Node::attach_protocol`, register for peer
  events with `Node::register_peer_events` (now returns
  `PeerEventListener`), and send or query peers through the `Node`.

### Added

- **`Node::broadcast`** (`karyon_p2p`): send to every connected peer on a
  protocol, from outside a protocol.
- **`SessionInfo` / `SessionInfoRef`** (`karyon_p2p`): what is known about
  one session with a peer (id, direction, endpoint, negotiated
  protocols). A peer that reconnects gets a new one.
- **`PeerConn::info`** (`karyon_p2p`): the session's `SessionInfo`.
- **`*Ref` aliases** (`karyon_p2p`): `NodeRef`, `ConfigRef`, `MonitorRef`,
  `DiscoveryRef`, `AccessControlRef`, `ConnectorRef`,
  `ListenerRef`, `protocol::ProtocolRef` and
  `protocol::ProtocolConstructorRef`, each an `Arc` of its type.

### Changed

- **`PeerConn::broadcast` after shutdown** (`karyon_p2p`): does nothing
  once the node is shut down, instead of panicking.
- **Connection slots** (`karyon_p2p`): a connection's inbound or outbound
  slot is freed once the peer is stopped, not when the listener or
  connector task notices it.
- **`ConnectionKind::Disconnected` for outbound peers** (`karyon_p2p`):
  carries the peer's remote socket address, like for inbound peers,
  instead of the dialed endpoint.
- **Discovery peer events** (`karyon_p2p`): `Discovery::on_event` gets
  peer events from the start of the pool, including those during
  shutdown, instead of only once the node runs its event task.
- **Shutdown order** (`karyon_p2p`): `Node::shutdown` stops the listener,
  the connector and the dials before the PeerPool, and the connections
  still queued for the pool emit `ConnectionKind::Disconnected`.

## karyon_core 1.1.3

### Added

- **`KeyPair::from_secret`** (`karyon_core`, feature `crypto`): load a
  keypair from the bytes `KeyPair::secret()` returns, so a node keeps
  the same keypair and PeerID across restarts.

## karyon_core 1.1.2, karyon_net 1.2.1, karyon_jsonrpc 1.1.5, karyon_kademlia 0.1.1, karyon_swarm 1.3.1, karyon_stun 0.2.3, karyon_eventemitter 1.0.4

### Added

- **`*Ref` type aliases** for types shared through `Arc`, e.g.
  `ServerRef = Arc<Server>`. They are plain aliases, so existing code
  keeps working.
  - `karyon_core`: `AsyncQueueRef<T>`, `CondWaitRef`, `TaskGroupRef`.
  - `karyon_net`: `WsLayerRef<C>`, `quic::QuicConnRef`.
  - `karyon_jsonrpc`: `client::ClientRef<B, W>`, `client::SubscriptionRef`,
    `server::ServerRef`, `server::ChannelRef`, `server::RPCServiceRef`,
    `server::PubSubRPCServiceRef`.
  - `karyon_kademlia`: `KademliaDiscoveryRef`, `KademliaConfigRef`.
  - `karyon_swarm`: `SwarmRef`.
  - `karyon_stun`: `StunServerRef`.
  - `karyon_eventemitter`: `EventEmitterRef<T>`, `AsEventValueRef`.

## karyon_p2p 1.3.0, karyon_kademlia 0.1.0, karyon_swarm 1.3.0

### Breaking changes

- **Kademlia moved to `karyon_kademlia`** (`karyon_p2p`): the Kademlia DHT
  discovery is now its own crate. `karyon_p2p::KademliaDiscovery` is now
  `karyon_kademlia::KademliaDiscovery`.
- **Kademlia settings moved to `KademliaConfig`** (`karyon_p2p`,
  `karyon_kademlia`): `discovery_endpoints`, `bootstrap_peers`,
  `public_ip`, `seeding_interval`, `lookup_*` and `refresh_*` are no
  longer `Config` fields; set them on `karyon_kademlia::KademliaConfig`.
- **`Node::new` no longer runs Kademlia** (`karyon_p2p`): it only dials
  `Config::dial_peers`. To find peers with Kademlia, use
  `Node::with_discovery(&key_pair, config, |ctx| KademliaDiscovery::new(ctx,
  kconfig), ex)`.
- **`Node::with_discovery` takes a constructor** (`karyon_p2p`): a closure
  that gets a `DiscoveryContext` (key pair, PeerID, config, monitor,
  executor) and returns the discovery, instead of a ready discovery.
- **`Discovery` trait** (`karyon_p2p`): the Node dials `Config::dial_peers`
  itself, so a discovery no longer yields them. `DiscoveredPeer` drops
  `discovery_addrs`. `PeerConnectionEvent::ConnectFailed` is split into
  `DialFailed` (the Node's dial failed) and `HandshakeFailed` (inbound or
  outbound, including `AccessControl` denials). `find_peers_with` is
  removed. `shutdown`, `on_event` and `advertise` now have empty default
  bodies.
- **`find_peers` removed** (`karyon_p2p`, `karyon_swarm`):
  `Node::find_peers_with` and `Swarm::find_peers` are gone. The peers they
  returned could not be dialed through any public API. Swarm keys are
  still advertised, and Kademlia still uses them to pick peers.

### Added

- **Building blocks for discovery crates** (`karyon_p2p`): `Connector`,
  `Listener`, `DiscoveryContext`, `PeerConnectionEvent` (needed to
  implement `Discovery` outside the crate), `pick_endpoint`, `ShutdownMsg`,
  `VersionInt`, `version_match`, and `Monitor::notify` with
  `monitor::{ConnectionKind, ToEvent}`. A discovery posts its own events
  as `DiscoveryEvent` by implementing `ToEvent`.
- **`karyon_kademlia`**: the Kademlia DHT discovery, with the peer and chat
  examples.

## karyon_p2p 1.2.5

### Fixes

- **Two nodes dialing each other** (`karyon_p2p`): when two nodes connect
  to each other at the same time, both now keep the same one of the two
  connections (the one dialed by the node with the smaller PeerID).
  Before, each could keep a different one and close the other's, so both
  lost the peer right after `PeerEvent::Added`. The node also no longer
  dials a discovered peer that is already connected.

## karyon_net 1.2.0, karyon_async_rustls 1.2.0, karyon_jsonrpc 1.1.4, karyon_p2p 1.2.4, karyon_stun 0.2.2

### Breaking changes

- **rustls crypto provider flag** (`karyon_net`, `karyon_async_rustls`): the
  provider is now chosen with the `aws-lc-rs` (default) or `ring` feature.
  With `default-features = false`, add one of them; the `quic` feature
  fails to build without one.

### Fixes

- **One rustls provider per build** (`karyon_net`): a build with TLS and
  QUIC compiled both aws-lc-rs and ring, so `rustls::ClientConfig::builder()`
  panicked unless a provider was installed first. A default build now
  compiles only aws-lc-rs.

## karyon_jsonrpc 1.1.3

### Fixes

- **Idle QUIC streams** (`karyon_jsonrpc`): with `ServerBuilder::read_timeout`
  set, the server drops a QUIC stream that does not send its request in
  time. Before, such a stream held a server task as long as the connection
  lived. Subscription streams waiting for their unsubscribe are not
  affected.
- **Codec on QUIC** (`karyon_jsonrpc`): QUIC now uses the codec given to
  `ServerBuilder::new_with_codec` / `ClientBuilder::new_with_codec`, and
  its size limit (`JsonCodec::new(max_size)`). Before, QUIC always used the
  default `JsonCodec`. A peer with a custom codec that relied on plain
  JSON over QUIC must now use the same codec on both sides.

## karyon_p2p 1.2.3

### Fixes

- **Handshake stalls** (`karyon_p2p`): every queued connection is now
  handshaked in its own task. Before, connections were handshaked one at a
  time, so a few connections that stayed silent kept honest peers from
  joining.
- **Dial stalls** (`karyon_p2p`): an outbound dial now fails after
  `handshake_timeout`, and discovered peers are dialed in their own tasks.
  Before, one endpoint that never answered the TLS handshake stopped all
  outbound dials.
- **QUIC stream setup** (`karyon_p2p`): a QUIC peer is added to the pool
  right after the handshake, and opens its protocol streams when it starts.
  A peer whose streams are not up within `handshake_timeout` is removed
  again, so `PeerEvent::Added` can be followed by `PeerEvent::Removed`.
  Before, the pool waited for the streams first.

## karyon_jsonrpc 1.1.2

### Fixes

- **Subscription streams on QUIC and HTTP/3** (`karyon_jsonrpc`):
  `Subscription::recv()` now returns an error when the server ends the
  subscription stream. Before, it waited forever and the client kept the
  subscription.

## karyon_net 1.1.2

### Breaking changes

- **`QuicConfig::read_chunk_size` removed** (`karyon_net`): the field was
  never used. Code that sets it must drop it.

### Fixes

- **IPv6 endpoints** (`karyon_net`): `tcp://[::1]:3000` now parses to an IP
  address, it was read as the domain `[::1]` and failed to resolve. An IPv6
  endpoint also prints with brackets, so it parses back.
- **Unix endpoints** (`karyon_net`): `Endpoint::Unix` prints as
  `unix:/path`. Before, it printed `unix://path`, which parsed back with
  the first path segment as the host.

## karyon_p2p 1.2.2

### Fixes

- **Decode limit** (`karyon_p2p`): decoding a message now fails when a
  length prefix inside it claims more than 4 MB. Before, one small frame
  with a huge length prefix made the node allocate that much memory and
  abort, before any authentication. It affected peer connections and the
  discovery lookup listener.

## karyon_core 1.1.1, karyon_net 1.1.1, karyon_jsonrpc 1.1.1, karyon_p2p 1.2.1, karyon_swarm 1.2.1, karyon_stun 0.2.1

### Fixes

- **Yanked `async-executor`**: the packaged `Cargo.lock` used
  `async-executor` 1.14.0, which is yanked on crates.io. It now uses 1.14.1.
  This only matters for builds that use the lock file, such as
  `cargo install --locked`.

## karyon_core 1.1.0, karyon_net 1.1.0, karyon_async_rustls 1.1.0, karyon_jsonrpc 1.1.0, karyon_jsonrpc_macro 1.1.0, karyon_p2p 1.2.0, karyon_swarm 1.2.0, karyon_stun 0.2.0, karyon_eventemitter 1.0.3, karyon_eventemitter_macro 1.0.2

### Breaking changes

- **tokio is the default runtime** (`karyon_core`, `karyon_net`,
  `karyon_async_rustls`, `karyon_jsonrpc`, `karyon_jsonrpc_macro`,
  `karyon_p2p`, `karyon_swarm`, `karyon_stun`): the default feature is now
  `tokio`, it was `smol`. To keep smol, use
  `default-features = false, features = ["smol"]`. `features = ["smol"]`
  alone turns on both runtimes and fails to compile.
- **Rust 2024 edition** (all crates): building karyon now needs Rust 1.85
  or newer.
- **Request limit per connection** (`karyon_jsonrpc`): a server handles at
  most 64 requests of one TCP, TLS, WebSocket or Unix connection at the
  same time; the connection is not read further until one finishes.

### Added

- **Client `HttpConfig`** (`karyon_jsonrpc`): `ClientBuilder::http_config`
  sets `http2` (HTTP/2 over plain TCP with prior knowledge), `host` (the
  `Host` header of HTTP/1.1 requests) and `content_type`.
- **Server `HttpConfig`** (`karyon_jsonrpc`): `ServerBuilder::http_config`
  sets the `content_type` of the responses.
- **`ClientBuilder::with_notification_decoder`** (`karyon_jsonrpc`): reads
  notifications sent in a custom shape by a server that uses
  `ServerBuilder::with_notification_encoder`.
- **HTTP examples** (`karyon_jsonrpc`): `examples/http_server.rs` and
  `examples/http_client.py`.

### Changed

- **Caret versions between karyon crates**: karyon crates no longer pin
  each other to an exact version, so a patch release of one crate can be
  used without new releases of the crates that depend on it.
- **`Host` header** (`karyon_jsonrpc`): the HTTP/1.1 client sends it only
  when `HttpConfig::host` is set.

### Fixes

- **Error responses on multiplexed clients** (`karyon_jsonrpc`): a failed
  `call` returns `Error::CallError` and a failed `subscribe` or
  `unsubscribe` returns `Error::SubscribeError`.
- **HTTP/3 pub/sub** (`karyon_jsonrpc`): each subscription has its own
  stream, as on QUIC, and `unsubscribe` finds the subscription.
- **Server connection handling** (`karyon_jsonrpc`): the HTTP/1.1 header
  read timeout now applies, request tasks stop when the connection writer
  stops, and accept loops back off on errors.

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
