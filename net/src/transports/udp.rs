use std::net::SocketAddr;

use async_trait::async_trait;
use smol::net::UdpSocket;

use crate::{
    connection::Connection,
    endpoint::{Addr, Endpoint, Port},
    Error, Result,
};

/// UDP network connection implementations of the [`Connection`] trait.
pub struct UdpConn {
    inner: UdpSocket,
}

impl UdpConn {
    /// Creates a new UdpConn
    pub fn new(conn: UdpSocket) -> Self {
        Self { inner: conn }
    }
}

impl UdpConn {
    /// Receives a single datagram message. Returns the number of bytes read
    /// and the origin endpoint.
    pub async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, Endpoint)> {
        let (size, addr) = self.inner.recv_from(buf).await?;
        Ok((size, Endpoint::new_udp_addr(&addr)))
    }

    /// Sends data to the given address. Returns the number of bytes written.
    pub async fn send_to(&self, buf: &[u8], addr: &Endpoint) -> Result<usize> {
        let addr: SocketAddr = addr.clone().try_into()?;
        let size = self.inner.send_to(buf, addr).await?;
        Ok(size)
    }
}

#[async_trait]
impl Connection for UdpConn {
    fn peer_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_udp_addr(&self.inner.peer_addr()?))
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_udp_addr(&self.inner.local_addr()?))
    }

    async fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.inner.recv(buf).await.map_err(Error::from)
    }

    async fn write(&self, buf: &[u8]) -> Result<usize> {
        self.inner.send(buf).await.map_err(Error::from)
    }
}

/// Connects to the given UDP address and port.
pub async fn dial_udp(addr: &Addr, port: &Port) -> Result<UdpConn> {
    let address = format!("{}:{}", addr, port);

    // Let the operating system assign an available port to this socket
    let conn = UdpSocket::bind("[::]:0").await?;
    conn.connect(address).await?;
    Ok(UdpConn::new(conn))
}

/// Listens on the given UDP address and port.
pub async fn listen_udp(addr: &Addr, port: &Port) -> Result<UdpConn> {
    let address = format!("{}:{}", addr, port);
    let conn = UdpSocket::bind(address).await?;
    let udp_conn = UdpConn::new(conn);
    Ok(udp_conn)
}
