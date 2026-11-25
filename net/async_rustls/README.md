# karyon async rustls

Async TLS for Rust with one rustls API over smol (futures-rustls) and tokio (tokio-rustls). An
internal companion crate of [karyon_net](https://crates.io/crates/karyon_net).

It re-exports `TlsAcceptor`, `TlsConnector`, `TlsStream` and `rustls` from the crate of the enabled
runtime, so the same names work on either runtime.

Use karyon_net with its `tls` feature instead of depending on this crate directly. karyon_net
re-exports it as `karyon_net::async_rustls` with the `tls` or `quic` feature.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `tokio` | Use tokio-rustls (default) |
| `smol` | Use futures-rustls |
| `aws-lc-rs` | rustls crypto provider aws-lc-rs (default) |
| `ring` | rustls crypto provider ring |

Exactly one runtime feature must be enabled. With no provider feature, every rustls config must be
built with an explicit provider.
