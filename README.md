# Karyon

## Overview

Many developers around the world aspire to build peer-to-peer, decentralized
apps that are resilient, secure, and free from central control.
However, there are still not many libraries and tools available to build these
kinds of apps. This forces many developers to either abandon their ideas or
develop a new p2p network stack and tools from scratch. Such efforts are not
only time-consuming but also prone to errors and security vulnerabilities, as
each new implementation reintroduces potential weaknesses.

Karyon provides developers with the components and tools needed to create
decentralized apps. By offering a robust infrastructure, Karyon simplifies the
complexities associated with building p2p apps. Karyon's primary goal
is to make the process of decentralization more achievable and efficient for
developers everywhere, pushing for a future where software is more open,
secure, and free from central control.

> In molecular biology, a Karyon is essentially "a part of the cell
> containing DNA and RNA and responsible for growth and reproduction"

## Join us

- [irc](irc://irc.libera.chat/#karyon) #karyon on Libera Chat
- [Discord](https://discord.gg/xuXRcrkz3p)

## Crates 

- [karyon core](./core):  Essential utilities and core functionality.
- [karyon net](./net): Provides a network interface for TCP, UDP, WebSocket, and Unix,
  along with common network functionality. 
- [karyon p2p](./p2p): A lightweight, extensible, and customizable
  peer-to-peer (p2p) network stack.
- [karyon jsonrpc](./jsonrpc): A fast and small async
  [JSONRPC2.0](https://www.jsonrpc.org/specification) implementation.
- karyon crdt: A [CRDT](https://en.wikipedia.org/wiki/Conflict-free_replicated_data_type) 
implementation for building collaborative software. 
- karyon base: A lightweight, extensible database that operates with `karyon crdt`.

## Choosing the async runtime

All the crates support both smol(async-std) and tokio. The default is smol, but
if you want to use tokio, you need to disable the default features and then
select the `tokio` feature.

## Docs

Online documentation for the main crates: 
[karyon_p2p](https://karyontech.github.io/karyon/karyon_p2p), 
[karyon_jsonrpc](https://karyontech.github.io/karyon/karyon_jsonrpc)

For the internal crates: 
[karyon_core](https://karyontech.github.io/karyon/karyon_core), 
[karyon_net](https://karyontech.github.io/karyon/karyon_net)

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
