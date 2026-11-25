# Karyon

[![Build](https://github.com/karyontech/karyon/actions/workflows/rust.yml/badge.svg)](https://github.com/karyontech/karyon/actions)
[![License](https://img.shields.io/crates/l/karyon_core)](https://github.com/karyontech/karyon/blob/master/LICENSE)

[![karyon_p2p crates.io](https://img.shields.io/crates/v/karyon_p2p?label=karyon_p2p%20crates.io)](https://crates.io/crates/karyon_p2p)
[![karyon_kademlia crates.io](https://img.shields.io/crates/v/karyon_kademlia?label=karyon_kademlia%20crates.io)](https://crates.io/crates/karyon_kademlia)
[![karyon_swarm crates.io](https://img.shields.io/crates/v/karyon_swarm?label=karyon_swarm%20crates.io)](https://crates.io/crates/karyon_swarm)
[![karyon_stun crates.io](https://img.shields.io/crates/v/karyon_stun?label=karyon_stun%20crates.io)](https://crates.io/crates/karyon_stun)
[![karyon_jsonrpc crates.io](https://img.shields.io/crates/v/karyon_jsonrpc?label=karyon_jsonrpc%20crates.io)](https://crates.io/crates/karyon_jsonrpc)
[![karyon_core crates.io](https://img.shields.io/crates/v/karyon_core?label=karyon_core%20crates.io)](https://crates.io/crates/karyon_core)
[![karyon_net crates.io](https://img.shields.io/crates/v/karyon_net?label=karyon_net%20crates.io)](https://crates.io/crates/karyon_net)
[![karyon_eventemitter crates.io](https://img.shields.io/crates/v/karyon_eventemitter?label=karyon_eventemitter%20crates.io)](https://crates.io/crates/karyon_eventemitter)

Composable async Rust crates for networking, peer-to-peer (p2p) networks, JSON-RPC, and
decentralized applications: a p2p node with Kademlia DHT discovery and swarms, STUN for NAT
traversal, a JSON-RPC 2.0 server and client, a layered transport stack, and a typed event emitter.

Crates that need an async runtime support tokio (default) and smol. To use smol, disable default
features and enable the `smol` feature.

[Website](https://karyontech.net/) | [Discord](https://discord.gg/xuXRcrkz3p) | [irc](https://libera.chat/) #karyon on liberachat

> In molecular biology, a Karyon is essentially "a part of the cell
> containing DNA and RNA and responsible for growth and reproduction"

## Crates

- **[karyon_core](./core)**: Async runtime abstraction (tokio, smol, or custom), executors, and shared utilities.
- **[karyon_net](./net)**: Layered networking stack over TCP, QUIC, UDP, and Unix sockets, with composable layers for TLS, WebSocket, and SOCKS5.
- **[karyon_p2p](./p2p)**: Extensible peer-to-peer networking stack with pluggable discovery, pluggable custom protocols, and support for TCP, TLS, and QUIC.
- **[karyon_kademlia](./kademlia)**: Kademlia DHT peer discovery for the p2p layer: routing table, lookups from bootstrap peers, and liveness refreshes over UDP or the lookup protocol.
- **[karyon_swarm](./swarm)**: Protocol-aware peer groups on top of the p2p layer, with BitTorrent-style swarm-keyed discovery.
- **[karyon_stun](./stun)**: Minimal async [STUN](https://www.rfc-editor.org/rfc/rfc5389) server and client over UDP, TCP, TLS, and QUIC (karyon extension), so peers behind NAT can discover their public address.
- **[karyon_jsonrpc](./jsonrpc)**: Lightweight async [JSON-RPC 2.0](https://www.jsonrpc.org/specification) implementation running over TCP, TLS, WebSocket, WSS, QUIC, HTTP/1.1, HTTP/2, HTTP/3, and Unix sockets.
- **[karyon_eventemitter](./utils/eventemitter)**: Lightweight, typed async event emitter for pub/sub and message dispatch.

## Custom Runtime, Transport and WASI

You can run the crates on your own runtime and over your own transport,
instead of tokio or smol and OS sockets. This is how they build for WASI
(`wasm32-wasip1`/`wasip2`). Every crate except karyon_stun builds for WASI.
See each crate's README.

## Docs

- [karyon_p2p](https://docs.rs/karyon_p2p/latest/karyon_p2p/)
- [karyon_kademlia](https://docs.rs/karyon_kademlia/latest/karyon_kademlia/)
- [karyon_swarm](https://docs.rs/karyon_swarm/latest/karyon_swarm/)
- [karyon_stun](https://docs.rs/karyon_stun/latest/karyon_stun/)
- [karyon_jsonrpc](https://docs.rs/karyon_jsonrpc/latest/karyon_jsonrpc/)
- [karyon_core](https://docs.rs/karyon_core/latest/karyon_core/)
- [karyon_net](https://docs.rs/karyon_net/latest/karyon_net/)
- [karyon_eventemitter](https://docs.rs/karyon_eventemitter/latest/karyon_eventemitter/)

## Contribution

Feel free to open a pull request or an [issue](https://github.com/karyontech/karyon/issues/new).

## License

All the code in this repository is licensed under the MIT License. You can
find a copy of the license in the [LICENSE](./LICENSE) file.
