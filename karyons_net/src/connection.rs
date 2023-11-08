use crate::{Endpoint, Result};
use async_trait::async_trait;

use crate::transports::{tcp, udp, unix};

/// Alias for `Box<dyn Connection>`
pub type Conn = Box<dyn Connection>;

/// Connection is a generic network connection interface for
/// `UdpConn`, `TcpConn`, and `UnixConn`.
///
/// If you are familiar with the Go language, this is similar to the `Conn`
/// interface <https://pkg.go.dev/net#Conn>
#[async_trait]
pub trait Connection: Send + Sync {
    /// Returns the remote peer endpoint of this connection
    fn peer_endpoint(&self) -> Result<Endpoint>;

    /// Returns the local socket endpoint of this connection
    fn local_endpoint(&self) -> Result<Endpoint>;

    /// Reads data from this connection.  
    async fn recv(&self, buf: &mut [u8]) -> Result<usize>;

    /// Sends data to this connection
    async fn send(&self, buf: &[u8]) -> Result<usize>;
}

/// Connects to the provided endpoint.
///
/// it only supports `tcp4/6`, `udp4/6` and `unix`.
///
/// #Example
///
/// ```
/// use karyons_net::{Endpoint, dial};
///
/// async {
///     let endpoint: Endpoint = "tcp://127.0.0.1:3000".parse().unwrap();
///
///     let conn = dial(&endpoint).await.unwrap();
///
///     conn.send(b"MSG").await.unwrap();
///
///     let mut buffer = [0;32];
///     conn.recv(&mut buffer).await.unwrap();
/// };
///
/// ```
///
pub async fn dial(endpoint: &Endpoint) -> Result<Conn> {
    match endpoint {
        Endpoint::Tcp(addr, port) => Ok(Box::new(tcp::dial_tcp(addr, port).await?)),
        Endpoint::Udp(addr, port) => Ok(Box::new(udp::dial_udp(addr, port).await?)),
        Endpoint::Unix(addr) => Ok(Box::new(unix::dial_unix(addr).await?)),
    }
}
