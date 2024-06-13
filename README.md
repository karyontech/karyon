# Karyon

A library for building p2p, decentralized, and collaborative software

[Website](https://karyontech.net/) | [Discord](https://discord.gg/xuXRcrkz3p) | [irc](https://libera.chat/) #karyon on liberachat 

> In molecular biology, a Karyon is essentially "a part of the cell
> containing DNA and RNA and responsible for growth and reproduction"

## Overview

Building peer-to-peer (p2p), decentralized applications that are resilient,
secure, and free from central control is a challenge for developers. There are
not many libraries and tools available to build these applications. As
a result, many developers either abandon their ideas or have to develop a new
p2p network stack and tools from scratch. Instead of sharing common components
and tools for building p2p systems, every p2p project seems to reinvent the
wheel, which increases the effort required and the potential for vulnerabilities.

Karyon addresses this issue by providing developers with the components and
tools needed to create p2p and decentralized apps, simplifying the complexities
associated with building them. its primary goal is to make decentralization
more accessible and efficient for developers everywhere.

## Crates 

- **[karyon core](./core)**:  Essential utilities and core functionality.
- **[karyon net](./net)**: Provides a network interface for TCP, UDP, TLS, WebSocket, and Unix,
  along with common network functionality. 
- **[karyon p2p](./p2p)**: A lightweight, extensible, and customizable
  peer-to-peer (p2p) network stack.
- **[karyon jsonrpc](./jsonrpc)**: A fast and lightweight async
  [JSONRPC2.0](https://www.jsonrpc.org/specification) implementation.
- **karyon crdt**: A [CRDT](https://en.wikipedia.org/wiki/Conflict-free_replicated_data_type) 
implementation for building collaborative software. 
- **karyon base**: A lightweight, extensible database that operates with **karyon crdt**.

## Choosing the async runtime

All the crates support both **smol(async-std)** and **tokio** async runtimes. 
The default is **smol**, but if you want to use **tokio**, you need to disable 
the default features and then select the `tokio` feature.

## Docs

Online documentation for the main crates: 
- [karyon_p2p](https://karyontech.github.io/karyon/karyon_p2p), 
- [karyon_jsonrpc](https://karyontech.github.io/karyon/karyon_jsonrpc)

For the internal crates: 
- [karyon_core](https://karyontech.github.io/karyon/karyon_core), 
- [karyon_net](https://karyontech.github.io/karyon/karyon_net)

## Status

This project is a work in progress. The current focus is on shipping `karyon
crdt` and `karyon base`, along with major changes to the network stack. You can
check the [issues](https://github.com/karyontech/karyon/issues) for updates on
ongoing tasks.

## Contribution

Feel free to open a pull request or an [issue](https://github.com/karyontech/karyon/issues/new). 

## License

All the code in this repository is licensed under the GNU General Public
License, version 3 (GPL-3.0). You can find a copy of the license in the
[LICENSE](./LICENSE) file.
