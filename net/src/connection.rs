use async_trait::async_trait;

use crate::{
    transports::{tcp, udp, unix},
    Endpoint, Error, Result,
};

/// Alias for `Box<dyn Connection>`
pub type Conn = Box<dyn Connection>;

/// A trait for objects which can be converted to [`Conn`].
pub trait ToConn {
    fn to_conn(self) -> Conn;
}

/// Connection is a generic network connection interface for
/// [`udp::UdpConn`], [`tcp::TcpConn`], and [`unix::UnixConn`].
///
/// If you are familiar with the Go language, this is similar to the
/// [Conn](https://pkg.go.dev/net#Conn) interface
#[async_trait]
pub trait Connection: Send + Sync {
    /// Returns the remote peer endpoint of this connection
    fn peer_endpoint(&self) -> Result<Endpoint>;

    /// Returns the local socket endpoint of this connection
    fn local_endpoint(&self) -> Result<Endpoint>;

    /// Reads data from this connection.  
    async fn read(&self, buf: &mut [u8]) -> Result<usize>;

    /// Writes data to this connection
    async fn write(&self, buf: &[u8]) -> Result<usize>;
}

/// Connects to the provided endpoint.
///
/// it only supports `tcp4/6`, `udp4/6`, and `unix`.
///
/// #Example
///
/// ```
/// use karyon_net::{Endpoint, dial};
///
/// async {
///     let endpoint: Endpoint = "tcp://127.0.0.1:3000".parse().unwrap();
///
///     let conn = dial(&endpoint).await.unwrap();
///
///     conn.write(b"MSG").await.unwrap();
///
///     let mut buffer = [0;32];
///     conn.read(&mut buffer).await.unwrap();
/// };
///
/// ```
///
pub async fn dial(endpoint: &Endpoint) -> Result<Conn> {
    match endpoint {
        Endpoint::Tcp(_, _) => Ok(Box::new(tcp::dial_tcp(endpoint).await?)),
        Endpoint::Udp(_, _) => Ok(Box::new(udp::dial_udp(endpoint).await?)),
        Endpoint::Unix(addr) => Ok(Box::new(unix::dial_unix(addr).await?)),
        _ => Err(Error::InvalidEndpoint(endpoint.to_string())),
    }
}
