use async_trait::async_trait;

use crate::{Conn, Endpoint, Result};

/// Alias for `Box<dyn ConnListener>`
pub type Listener<T> = Box<dyn ConnListener<Item = T>>;

/// A trait for objects which can be converted to [`Listener`].
pub trait ToListener {
    type Item;
    fn to_listener(self) -> Listener<Self::Item>;
}

/// ConnListener is a generic network listener interface for
/// [`tcp::TcpConn`], [`tls::TlsConn`], [`ws::WsConn`], and [`unix::UnixConn`].
#[async_trait]
pub trait ConnListener: Send + Sync {
    type Item;
    fn local_endpoint(&self) -> Result<Endpoint>;
    async fn accept(&self) -> Result<Conn<Self::Item>>;
}
