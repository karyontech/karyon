# karyon stun

A minimal async STUN ([RFC 5389](https://www.rfc-editor.org/rfc/rfc5389))
server and client over UDP, TCP, TLS and QUIC, built on
[karyon_net](https://crates.io/crates/karyon_net).
Binding requests and responses only - no TURN, no ICE, no authentication.
Useful for peers to discover their public address behind NAT.

## Install

```bash
$ cargo add karyon_stun
```

## Example

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

## Feature Flags

| Feature | Description |
|---------|-------------|
| `udp` | UDP transport, the primary STUN transport (default) |
| `tcp` | TCP transport (default) |
| `tls` | TLS over TCP transport (default) |
| `quic` | QUIC transport, one bidirectional stream per query (default) |
| `smol` | Use smol async runtime (default) |
| `tokio` | Use tokio async runtime |

TLS and QUIC take a karyon_net config on both sides: set
`StunConfig::tls_endpoint` / `quic_endpoint` on the server, and call
`StunClient::with_tls` / `with_quic` on the client.

## Scope

Implements the useful subset of RFC 5389:

- Binding Request / Response / Error Response
- XOR-MAPPED-ADDRESS (preferred) and MAPPED-ADDRESS (legacy fallback)
- ERROR-CODE and SOFTWARE attributes
- UDP retransmits (500ms / 1s / 2s), multiple requests per TCP, TLS
  or QUIC stream
- Stream connections are capped (`StunConfig::max_connections`, default
  1024) and closed after 10s without a request

Out of scope: TURN relaying, ICE, hole punching, authentication
(MESSAGE-INTEGRITY), DNS SRV discovery. The server is stateless: it
answers every Binding Request with the source address it saw.
