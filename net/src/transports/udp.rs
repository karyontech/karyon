use std::net::SocketAddr;
use std::sync::Arc;

use karyon_core::async_runtime::net::UdpSocket;

use crate::{Endpoint, Result};

/// UDP config.
#[derive(Default)]
pub struct UdpConfig {}

/// Raw UDP connection. Sends and receives raw bytes with addresses.
pub struct UdpConn {
    socket: Arc<UdpSocket>,
}

impl UdpConn {
    /// Send raw bytes to an address.
    pub async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> Result<usize> {
        let n = self.socket.send_to(buf, addr).await?;
        Ok(n)
    }

    /// Receive raw bytes. Returns (bytes_read, sender address).
    pub async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        let (n, addr) = self.socket.recv_from(buf).await?;
        Ok((n, addr))
    }

    /// Local address this socket is bound to.
    pub fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_udp_addr(self.socket.local_addr()?))
    }

    /// Peer address (if connected via dial).
    pub fn peer_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_udp_addr(self.socket.peer_addr()?))
    }
}

/// Connect to a UDP endpoint.
pub async fn dial(endpoint: &Endpoint, _config: UdpConfig) -> Result<UdpConn> {
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let socket = UdpSocket::bind("[::]:0").await?;
    socket.connect(addr).await?;
    Ok(UdpConn {
        socket: Arc::new(socket),
    })
}

/// Listen on a UDP endpoint.
pub async fn listen(endpoint: &Endpoint, _config: UdpConfig) -> Result<UdpConn> {
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let socket = UdpSocket::bind(addr).await?;
    Ok(UdpConn {
        socket: Arc::new(socket),
    })
}
