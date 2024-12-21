use std::result::Result;

use async_trait::async_trait;

use crate::Endpoint;

/// Alias for `Box<dyn Connection>`
pub type Conn<C, E> = Box<dyn Connection<Message = C, Error = E>>;

/// A trait for objects which can be converted to [`Conn`].
pub trait ToConn {
    type Message;
    type Error;
    fn to_conn(self) -> Conn<Self::Message, Self::Error>;
}

/// Connection is a generic network connection interface for
/// [`udp::UdpConn`], [`tcp::TcpConn`], [`tls::TlsConn`], [`ws::WsConn`],
/// and [`unix::UnixConn`].
///
/// If you are familiar with the Go language, this is similar to the
/// [Conn](https://pkg.go.dev/net#Conn) interface
#[async_trait]
pub trait Connection: Send + Sync {
    type Message;
    type Error;
    /// Returns the remote peer endpoint of this connection
    fn peer_endpoint(&self) -> Result<Endpoint, Self::Error>;

    /// Returns the local socket endpoint of this connection
    fn local_endpoint(&self) -> Result<Endpoint, Self::Error>;

    /// Recvs data from this connection.  
    async fn recv(&self) -> Result<Self::Message, Self::Error>;

    /// Sends data to this connection
    async fn send(&self, msg: Self::Message) -> Result<(), Self::Error>;
}
