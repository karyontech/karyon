use std::result::Result;

use async_trait::async_trait;

use crate::{Conn, Endpoint};

/// Alias for `Box<dyn ConnListener>`
pub type Listener<C, E> = Box<dyn ConnListener<Message = C, Error = E>>;

/// A trait for objects which can be converted to [`Listener`].
pub trait ToListener {
    type Message;
    type Error;
    fn to_listener(self) -> Listener<Self::Message, Self::Error>;
}

/// ConnListener is a generic network listener interface for
/// [`tcp::TcpConn`], [`tls::TlsConn`], [`ws::WsConn`], and [`unix::UnixConn`].
#[async_trait]
pub trait ConnListener: Send + Sync {
    type Message;
    type Error;
    fn local_endpoint(&self) -> Result<Endpoint, Self::Error>;
    async fn accept(&self) -> Result<Conn<Self::Message, Self::Error>, Self::Error>;
}
