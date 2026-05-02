# Karyon

[![Build](https://github.com/karyontech/karyon/actions/workflows/rust.yml/badge.svg)](https://github.com/karyontech/karyon/actions)
[![License](https://img.shields.io/crates/l/karyon_core)](https://github.com/karyontech/karyon/blob/master/LICENSE)

- [![karyon_jsonrpc crates.io](https://img.shields.io/crates/v/karyon_jsonrpc?label=karyon_jsonrpc%20crates.io)](https://crates.io/crates/karyon_jsonrpc)
- [![karyon_jsonrpc docs.rs](https://img.shields.io/docsrs/karyon_jsonrpc?label=karyon_jsonrpc%20docs.rs)](https://docs.rs/karyon_jsonrpc/latest/karyon_jsonrpc/)

**The DNA of Decentralized Apps**

A set of composable Rust crates for networking and decentralized software:
pluggable peer discovery (Kademlia, mDNS, or custom), secure multi-transport
networking (TCP, TLS, WebSocket, WSS, QUIC, UDP, Unix, SOCKS5), JSON-RPC over
any transport, and protocol-aware swarms with swarm-keyed discovery.

All crates support both smol (default) and tokio. To use tokio, disable
default features and enable the `tokio` feature.

[Website](https://karyontech.net/) | [Discord](https://discord.gg/xuXRcrkz3p) | [irc](https://libera.chat/) #karyon on liberachat

> In molecular biology, a Karyon is essentially "a part of the cell
> containing DNA and RNA and responsible for growth and reproduction"

## Crates

- **[karyon core](./core)**: Essential utilities and core functionality.
- **[karyon net](./net)**: Layered network transport library with composable middleware (TCP, TLS, WebSocket, QUIC, SOCKS5, Unix, UDP).
- **[karyon p2p](./p2p)**: A lightweight, extensible, and customizable p2p network stack.
- **[karyon swarm](./swarm)**: Swarm layer on top of karyon_p2p, protocol-aware peer groups with swarm-keyed discovery.
- **[karyon jsonrpc](./jsonrpc)**: A fast and lightweight async
  [JSONRPC2.0](https://www.jsonrpc.org/specification) implementation with HTTP/1.1, HTTP/2, HTTP/3, WebSocket, QUIC, and TCP/TLS support.
- **[karyon eventemitter](./utils/eventemitter)**: A lightweight, asynchronous event emitter.

## Docs

- [karyon_p2p](https://karyontech.github.io/karyon/karyon_p2p)
- [karyon_swarm](https://karyontech.github.io/karyon/karyon_swarm)
- [karyon_jsonrpc](https://karyontech.github.io/karyon/karyon_jsonrpc)
- [karyon_core](https://karyontech.github.io/karyon/karyon_core)
- [karyon_net](https://karyontech.github.io/karyon/karyon_net)
- [karyon_eventemitter](https://karyontech.github.io/karyon/karyon_eventemitter)

## Contribution

Feel free to open a pull request or an [issue](https://github.com/karyontech/karyon/issues/new).

## License

All the code in this repository is licensed under the MIT License. You can
find a copy of the license in the [LICENSE](./LICENSE) file.
