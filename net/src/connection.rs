use async_trait::async_trait;

use crate::{Endpoint, Result};

/// Alias for `Box<dyn Connection>`
pub type Conn<T> = Box<dyn Connection<Item = T>>;

/// A trait for objects which can be converted to [`Conn`].
pub trait ToConn {
    type Item;
    fn to_conn(self) -> Conn<Self::Item>;
}

/// Connection is a generic network connection interface for
/// [`udp::UdpConn`], [`tcp::TcpConn`], [`tls::TlsConn`], [`ws::WsConn`],
/// and [`unix::UnixConn`].
///
/// If you are familiar with the Go language, this is similar to the
/// [Conn](https://pkg.go.dev/net#Conn) interface
#[async_trait]
pub trait Connection: Send + Sync {
    type Item;
    /// Returns the remote peer endpoint of this connection
    fn peer_endpoint(&self) -> Result<Endpoint>;

    /// Returns the local socket endpoint of this connection
    fn local_endpoint(&self) -> Result<Endpoint>;

    /// Recvs data from this connection.  
    async fn recv(&self) -> Result<Self::Item>;

    /// Sends data to this connection
    async fn send(&self, msg: Self::Item) -> Result<()>;
}
