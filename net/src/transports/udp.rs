use std::net::SocketAddr;

use async_trait::async_trait;
use karyon_core::async_runtime::net::UdpSocket;

use crate::{
    codec::Codec,
    connection::{Conn, Connection, ToConn},
    endpoint::Endpoint,
    Error, Result,
};

const BUFFER_SIZE: usize = 64 * 1024;

/// UDP configuration
#[derive(Default)]
pub struct UdpConfig {}

/// UDP network connection implementation of the [`Connection`] trait.
#[allow(dead_code)]
pub struct UdpConn<C> {
    inner: UdpSocket,
    codec: C,
    config: UdpConfig,
}

impl<C> UdpConn<C>
where
    C: Codec + Clone,
{
    /// Creates a new UdpConn
    fn new(socket: UdpSocket, config: UdpConfig, codec: C) -> Self {
        Self {
            inner: socket,
            codec,
            config,
        }
    }
}

#[async_trait]
impl<C> Connection for UdpConn<C>
where
    C: Codec + Clone,
{
    type Item = (C::Item, Endpoint);
    fn peer_endpoint(&self) -> Result<Endpoint> {
        self.inner
            .peer_addr()
            .map(Endpoint::new_udp_addr)
            .map_err(Error::from)
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        self.inner
            .local_addr()
            .map(Endpoint::new_udp_addr)
            .map_err(Error::from)
    }

    async fn recv(&self) -> Result<Self::Item> {
        let mut buf = [0u8; BUFFER_SIZE];
        let (_, addr) = self.inner.recv_from(&mut buf).await?;
        match self.codec.decode(&mut buf)? {
            Some((_, item)) => Ok((item, Endpoint::new_udp_addr(addr))),
            None => Err(Error::Decode("Unable to decode".into())),
        }
    }

    async fn send(&self, msg: Self::Item) -> Result<()> {
        let (msg, out_addr) = msg;
        let mut buf = [0u8; BUFFER_SIZE];
        self.codec.encode(&msg, &mut buf)?;
        let addr: SocketAddr = out_addr.try_into()?;
        self.inner.send_to(&buf, addr).await?;
        Ok(())
    }
}

/// Connects to the given UDP address and port.
pub async fn dial<C>(endpoint: &Endpoint, config: UdpConfig, codec: C) -> Result<UdpConn<C>>
where
    C: Codec + Clone,
{
    let addr = SocketAddr::try_from(endpoint.clone())?;

    // Let the operating system assign an available port to this socket
    let conn = UdpSocket::bind("[::]:0").await?;
    conn.connect(addr).await?;
    Ok(UdpConn::new(conn, config, codec))
}

/// Listens on the given UDP address and port.
pub async fn listen<C>(endpoint: &Endpoint, config: UdpConfig, codec: C) -> Result<UdpConn<C>>
where
    C: Codec + Clone,
{
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let conn = UdpSocket::bind(addr).await?;
    Ok(UdpConn::new(conn, config, codec))
}

impl<C> ToConn for UdpConn<C>
where
    C: Codec + Clone,
{
    type Item = (C::Item, Endpoint);
    fn to_conn(self) -> Conn<Self::Item> {
        Box::new(self)
    }
}
