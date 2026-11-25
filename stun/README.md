# karyon stun

A minimal async [STUN](https://www.rfc-editor.org/rfc/rfc5389) (RFC 5389) server and client in
Rust, built on [karyon_net](https://crates.io/crates/karyon_net). Peers behind NAT use it to
discover their public IP address and port.

It runs over UDP, TCP, and TLS per RFC 5389, and over QUIC as a karyon extension (see
[STUN over QUIC](#stun-over-quic)).

Use cases:

- Learn a node's public address before advertising it, e.g. `KademliaConfig::public_ip` in
  karyon_kademlia
- Run a STUN server for your own peers
- Query standard STUN servers over UDP, TCP, or TLS

## Install

```bash
$ cargo add karyon_stun
```

## Example

### Server

```rust,no_run
use karyon_core::async_runtime::global_executor;
use karyon_stun::{StunConfig, StunServer};

async {
    let config = StunConfig {
        udp_endpoint: Some("udp://0.0.0.0:3478".parse().unwrap()),
        tcp_endpoint: Some("tcp://0.0.0.0:3478".parse().unwrap()),
        software: Some("karyon-stun".into()),
        ..Default::default()
    };

    let server = StunServer::new(config, global_executor());
    server.run().await.expect("run stun server");

    // ... until shutdown:
    server.shutdown().await;
};
```

The server is stateless. It replies to each Binding Request with the source address it saw.
Endpoints left as `None` are not served. `run` binds every configured endpoint before it returns
and serves in background tasks.

### Client

```rust,no_run
use karyon_net::Endpoint;
use karyon_stun::StunClient;

async {
    let server: Endpoint = "udp://198.51.100.7:3478".parse().unwrap();
    let client = StunClient::new(server);
    let public_addr = client.discover().await.expect("discover public address");
    println!("public address: {public_addr}");
};
```

The server endpoint must use an IP address. Hostname resolution is up to the caller. Over UDP,
`discover_from(local)` binds the query socket to `local`, to learn the public mapping of that
socket.

TLS and QUIC take a karyon_net config on both sides. Set `StunConfig::tls_endpoint` or
`quic_endpoint` on the server, and call `StunClient::with_tls` or `with_quic` on the client.

## STUN over QUIC

QUIC is not part of RFC 5389 or 8489. It is a karyon extension that carries STUN messages on one
QUIC bidirectional stream. It works between karyon servers and clients only. Standard STUN servers
and clients do not speak it.

```rust,no_run
# use karyon_core::async_runtime::global_executor;
# use karyon_net::quic::{ClientQuicConfig, ServerQuicConfig};
# use karyon_net::Endpoint;
# use karyon_stun::{StunClient, StunConfig, StunServer};
# async fn _ex(server_config: ServerQuicConfig, client_config: ClientQuicConfig) {
let endpoint: Endpoint = "quic://0.0.0.0:5349".parse().unwrap();
let config = StunConfig {
    quic_endpoint: Some((endpoint, server_config)),
    ..Default::default()
};
let server = StunServer::new(config, global_executor());
server.run().await.expect("run stun server");

let client = StunClient::new("quic://198.51.100.7:5349".parse().unwrap())
    .with_quic(client_config);
let public_addr = client.discover().await.expect("discover public address");
println!("public address: {public_addr}");
# }
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `udp` | UDP, the primary STUN transport (default) |
| `tcp` | TCP (default) |
| `tls` | TLS over TCP (implies `tcp`, default) |
| `quic` | QUIC, karyon extension (default) |
| `tokio` | Use tokio async runtime (default) |
| `smol` | Use smol async runtime |
| `aws-lc-rs` | rustls crypto provider: aws-lc-rs (default) |
| `ring` | rustls crypto provider: ring. Use with `default-features = false` |

`quic` needs one of the provider features.

```toml
# Default (tokio + UDP/TCP/TLS/QUIC)
karyon_stun = "0.4"

# UDP only
karyon_stun = { version = "0.4", default-features = false, features = ["tokio", "udp"] }

# With smol
karyon_stun = { version = "0.4", default-features = false, features = ["smol", "udp", "tcp", "tls"] }
```

karyon_stun does not build for wasm.

## Scope

It implements the useful subset of RFC 5389:

- Binding Request, Response, and Error Response
- XOR-MAPPED-ADDRESS (preferred) and MAPPED-ADDRESS (legacy fallback)
- ERROR-CODE and SOFTWARE attributes
- UDP retransmits (500ms, 1s, 2s), and multiple requests per TCP, TLS, or QUIC stream

Stream connections are capped by `StunConfig::max_connections` (default 1024, UDP is not limited).
Extra connections are dropped on accept. The server closes a connection after 10s without a
request. The client gives up on a TCP, TLS, or QUIC query after 5s.
