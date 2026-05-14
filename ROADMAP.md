# Roadmap

This document tracks planned features beyond the initial 1.0 release. 

## mDNS discovery

Local network discovery via an mDNS implementation of the `Discovery` trait.

## Access control

For running karyon in private or semi-private deployments. Can be combined with
any discovery backend.

## Additional crates

### `karyon_stun`

Minimal STUN server and client. Multi-transport (UDP primary, plus
TCP, TLS, QUIC) built on `karyon_net`.

Useful for peers to discover their public address behind NAT.

### `karyon_pow`

Proof-of-work gate for inbound connections. Uses Equi-X (the same puzzle
algorithm as Tor / Arti).

Simple challenge/response protocol: server sends a random seed, client
solves the puzzle, server verifies, then the normal handshake proceeds.

References:
- [Tor's PoW defense against DoS attacks](https://blog.torproject.org/introducing-proof-of-work-defense-for-onion-services/)
- [Equi-X specification](https://spec.torproject.org/hspow-spec/v1-equix.html)
- [`equix` crate](https://crates.io/crates/equix)
