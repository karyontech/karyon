# Karyon

[![Build](https://github.com/karyontech/karyon/actions/workflows/rust.yml/badge.svg)](https://github.com/karyontech/karyon/actions)
[![License](https://img.shields.io/crates/l/karyon_core)](https://github.com/karyontech/karyon/blob/master/LICENSE)

[![karyon_p2p crates.io](https://img.shields.io/crates/v/karyon_p2p?label=karyon_p2p%20crates.io)](https://crates.io/crates/karyon_p2p)
[![karyon_swarm crates.io](https://img.shields.io/crates/v/karyon_swarm?label=karyon_swarm%20crates.io)](https://crates.io/crates/karyon_swarm)
[![karyon_jsonrpc crates.io](https://img.shields.io/crates/v/karyon_jsonrpc?label=karyon_jsonrpc%20crates.io)](https://crates.io/crates/karyon_jsonrpc)
[![karyon_core crates.io](https://img.shields.io/crates/v/karyon_core?label=karyon_core%20crates.io)](https://crates.io/crates/karyon_core)
[![karyon_net crates.io](https://img.shields.io/crates/v/karyon_net?label=karyon_net%20crates.io)](https://crates.io/crates/karyon_net)
[![karyon_eventemitter crates.io](https://img.shields.io/crates/v/karyon_eventemitter?label=karyon_eventemitter%20crates.io)](https://crates.io/crates/karyon_eventemitter)

A set of composable Rust crates for networking, peer-to-peer systems, and decentralized software.

`karyon_net` provides a layered async networking stack with composable
transports and middleware, supporting TCP, QUIC, UDP, Unix sockets, TLS,
WebSocket, and SOCKS5 proxying.

`karyon_jsonrpc` offers a lightweight async JSON-RPC 2.0 implementation with
support for stream transports, QUIC multiplexing, and native HTTP/1.1, HTTP/2,
and HTTP/3 communication.

`karyon_p2p` delivers an extensible peer-to-peer stack with pluggable
discovery, negotiated protocols, secure peer identity, and multi-endpoint
connectivity over TCP, TLS, and QUIC.

`karyon_swarm` builds on the p2p layer with protocol-aware swarms and
BitTorrent-style swarm-keyed discovery for scalable decentralized peer
coordination.

All crates support both smol (default) and tokio. To use tokio, disable
default features and enable the `tokio` feature.

[Website](https://karyontech.net/) | [Discord](https://discord.gg/xuXRcrkz3p) | [irc](https://libera.chat/) #karyon on liberachat

> In molecular biology, a Karyon is essentially "a part of the cell
> containing DNA and RNA and responsible for growth and reproduction"

## Crates

- **[karyon_core](./core)**: Core utilities and shared foundational components.
- **[karyon_net](./net)**: Layered networking stack with composable transports and middleware, including TCP, QUIC, Unix sockets, TLS, WebSocket, SOCKS5, and UDP.
- **[karyon_p2p](./p2p)**: Extensible peer-to-peer networking stack with pluggable discovery, negotiated protocols, and multi-endpoint connectivity over TCP, TLS, and QUIC.
- **[karyon_swarm](./swarm)**: Builds on the p2p layer with protocol-aware swarms and BitTorrent-style swarm-keyed discovery for scalable decentralized peer coordination.
- **[karyon_jsonrpc](./jsonrpc)**: Lightweight async [JSON-RPC 2.0](https://www.jsonrpc.org/specification) framework supporting TCP, TLS, WebSocket, QUIC, HTTP/1.1, HTTP/2, HTTP/3, and Unix transports.
- **[karyon_eventemitter](./utils/eventemitter)**: Lightweight asynchronous event emitter for strongly typed pub/sub communication.

## Docs

- [karyon_p2p](https://docs.rs/karyon_p2p/latest/karyon_p2p/)
- [karyon_swarm](https://docs.rs/karyon_swarm/latest/karyon_swarm/)
- [karyon_jsonrpc](https://docs.rs/karyon_jsonrpc/latest/karyon_jsonrpc/)
- [karyon_core](https://docs.rs/karyon_core/latest/karyon_core/)
- [karyon_net](https://docs.rs/karyon_net/latest/karyon_net/)
- [karyon_eventemitter](https://docs.rs/karyon_eventemitter/latest/karyon_eventemitter/)

## Contribution

Feel free to open a pull request or an [issue](https://github.com/karyontech/karyon/issues/new).

## License

All the code in this repository is licensed under the MIT License. You can
find a copy of the license in the [LICENSE](./LICENSE) file.
