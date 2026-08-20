# karyon async rustls

Internal companion crate of [karyon_net](https://crates.io/crates/karyon_net):
a thin async rustls wrapper that works with both smol (via
futures-rustls) and tokio (via tokio-rustls) behind one API.

You almost certainly want [karyon_net](https://crates.io/crates/karyon_net)
and its `tls` feature instead of depending on this crate directly.
