use std::net::SocketAddr;

use async_trait::async_trait;
use karyon_core::async_runtime::net::UdpSocket;

use crate::{
    codec::{Buffer, Codec},
    connection::{Conn, Connection, ToConn},
    endpoint::Endpoint,
    Result,
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
impl<C, E> Connection for UdpConn<C>
where
    C: Codec<Error = E> + Clone,
    E: From<std::io::Error>,
{
    type Message = (C::Message, Endpoint);
    type Error = E;
    fn peer_endpoint(&self) -> std::result::Result<Endpoint, Self::Error> {
        Ok(self.inner.peer_addr().map(Endpoint::new_udp_addr)?)
    }

    fn local_endpoint(&self) -> std::result::Result<Endpoint, Self::Error> {
        Ok(self.inner.local_addr().map(Endpoint::new_udp_addr)?)
    }

    async fn recv(&self) -> std::result::Result<Self::Message, Self::Error> {
        let mut buf = Buffer::new(BUFFER_SIZE);
        let (_, addr) = self.inner.recv_from(buf.as_mut()).await?;
        match self.codec.decode(&mut buf)? {
            Some((_, msg)) => Ok((msg, Endpoint::new_udp_addr(addr))),
            None => Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted).into()),
        }
    }

    async fn send(&self, msg: Self::Message) -> std::result::Result<(), Self::Error> {
        let (msg, out_addr) = msg;
        let mut buf = Buffer::new(BUFFER_SIZE);
        self.codec.encode(&msg, &mut buf)?;
        let addr: SocketAddr = out_addr
            .try_into()
            .map_err(|_| std::io::Error::other("Convert Endpoint to SocketAddress"))?;
        self.inner.send_to(buf.as_ref(), addr).await?;
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

impl<C, E> ToConn for UdpConn<C>
where
    C: Codec<Error = E> + Clone + 'static,
    E: From<std::io::Error>,
{
    type Message = (C::Message, Endpoint);
    type Error = E;
    fn to_conn(self) -> Conn<Self::Message, Self::Error> {
        Box::new(self)
    }
}
