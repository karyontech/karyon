# karyon stun

A minimal async [STUN](https://www.rfc-editor.org/rfc/rfc5389) server and
client, built on [karyon_net](https://crates.io/crates/karyon_net). Lets
peers behind NAT discover their public address. Runs over UDP, TCP, TLS
and QUIC: the first three per RFC 5389, QUIC as a karyon extension (see
[STUN over QUIC](#stun-over-quic)).

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

TLS and QUIC take a karyon_net config on both sides: set
`StunConfig::tls_endpoint` / `quic_endpoint` on the server, and call
`StunClient::with_tls` / `with_quic` on the client.

## STUN over QUIC

karyon_stun can also serve and query STUN over QUIC.

QUIC is not part of RFC 5389/8489: it is a karyon extension (STUN
messages on one QUIC bidirectional stream). It works between karyon
servers and clients; standard STUN servers and clients do not speak it.

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
| `smol` | Use smol async runtime (default) |
| `tokio` | Use tokio async runtime |

```toml
# Default (smol + UDP/TCP/TLS/QUIC)
karyon_stun = "0.1"

# UDP only
karyon_stun = { version = "0.1", default-features = false, features = ["smol", "udp"] }

# With tokio
karyon_stun = { version = "0.1", default-features = false, features = ["tokio", "udp", "tcp", "tls"] }
```

## Scope

Implements the useful subset of RFC 5389:

- Binding Request / Response / Error Response
- XOR-MAPPED-ADDRESS (preferred) and MAPPED-ADDRESS (legacy fallback)
- ERROR-CODE and SOFTWARE attributes
- UDP retransmits (500ms / 1s / 2s), multiple requests per TCP, TLS
  or QUIC stream
- Stream connections are capped (`StunConfig::max_connections`, default
  1024) and closed after 10s without a request
