use std::net::SocketAddr;

use async_trait::async_trait;
use futures_util::SinkExt;

use karyon_core::async_runtime::{
    io::{split, ReadHalf, WriteHalf},
    lock::Mutex,
    net::{TcpListener as AsyncTcpListener, TcpStream},
};

use crate::{
    codec::Codec,
    connection::{Conn, Connection, ToConn},
    endpoint::Endpoint,
    listener::{ConnListener, Listener, ToListener},
    stream::{ReadStream, WriteStream},
    Result,
};

/// TCP configuration
#[derive(Clone)]
pub struct TcpConfig {
    pub nodelay: bool,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self { nodelay: true }
    }
}

/// TCP connection implementation of the [`Connection`] trait.
pub struct TcpConn<C> {
    read_stream: Mutex<ReadStream<ReadHalf<TcpStream>, C>>,
    write_stream: Mutex<WriteStream<WriteHalf<TcpStream>, C>>,
    peer_endpoint: Endpoint,
    local_endpoint: Endpoint,
}

impl<C> TcpConn<C>
where
    C: Codec + Clone,
{
    /// Creates a new TcpConn
    pub fn new(
        socket: TcpStream,
        codec: C,
        peer_endpoint: Endpoint,
        local_endpoint: Endpoint,
    ) -> Self {
        let (read, write) = split(socket);
        let read_stream = Mutex::new(ReadStream::new(read, codec.clone()));
        let write_stream = Mutex::new(WriteStream::new(write, codec));
        Self {
            read_stream,
            write_stream,
            peer_endpoint,
            local_endpoint,
        }
    }
}

#[async_trait]
impl<C> Connection for TcpConn<C>
where
    C: Codec + Clone,
{
    type Item = C::Item;
    fn peer_endpoint(&self) -> Result<Endpoint> {
        Ok(self.peer_endpoint.clone())
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(self.local_endpoint.clone())
    }

    async fn recv(&self) -> Result<Self::Item> {
        self.read_stream.lock().await.recv().await
    }

    async fn send(&self, msg: Self::Item) -> Result<()> {
        self.write_stream.lock().await.send(msg).await
    }
}

pub struct TcpListener<C> {
    inner: AsyncTcpListener,
    config: TcpConfig,
    codec: C,
}

impl<C> TcpListener<C>
where
    C: Codec,
{
    pub fn new(listener: AsyncTcpListener, config: TcpConfig, codec: C) -> Self {
        Self {
            inner: listener,
            config: config.clone(),
            codec,
        }
    }
}

#[async_trait]
impl<C> ConnListener for TcpListener<C>
where
    C: Codec + Clone,
{
    type Item = C::Item;
    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_tcp_addr(self.inner.local_addr()?))
    }

    async fn accept(&self) -> Result<Conn<C::Item>> {
        let (socket, _) = self.inner.accept().await?;
        socket.set_nodelay(self.config.nodelay)?;

        let peer_endpoint = socket.peer_addr().map(Endpoint::new_tcp_addr)?;
        let local_endpoint = socket.local_addr().map(Endpoint::new_tcp_addr)?;

        Ok(Box::new(TcpConn::new(
            socket,
            self.codec.clone(),
            peer_endpoint,
            local_endpoint,
        )))
    }
}

/// Connects to the given TCP address and port.
pub async fn dial<C>(endpoint: &Endpoint, config: TcpConfig, codec: C) -> Result<TcpConn<C>>
where
    C: Codec + Clone,
{
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let socket = TcpStream::connect(addr).await?;
    socket.set_nodelay(config.nodelay)?;

    let peer_endpoint = socket.peer_addr().map(Endpoint::new_tcp_addr)?;
    let local_endpoint = socket.local_addr().map(Endpoint::new_tcp_addr)?;

    Ok(TcpConn::new(socket, codec, peer_endpoint, local_endpoint))
}

/// Listens on the given TCP address and port.
pub async fn listen<C>(endpoint: &Endpoint, config: TcpConfig, codec: C) -> Result<TcpListener<C>>
where
    C: Codec,
{
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let listener = AsyncTcpListener::bind(addr).await?;
    Ok(TcpListener::new(listener, config, codec))
}

impl<C> From<TcpListener<C>> for Box<dyn ConnListener<Item = C::Item>>
where
    C: Clone + Codec,
{
    fn from(listener: TcpListener<C>) -> Self {
        Box::new(listener)
    }
}

impl<C> ToConn for TcpConn<C>
where
    C: Codec + Clone,
{
    type Item = C::Item;
    fn to_conn(self) -> Conn<Self::Item> {
        Box::new(self)
    }
}

impl<C> ToListener for TcpListener<C>
where
    C: Clone + Codec,
{
    type Item = C::Item;
    fn to_listener(self) -> Listener<Self::Item> {
        self.into()
    }
}
