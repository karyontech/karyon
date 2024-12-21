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
impl<C, E> Connection for TcpConn<C>
where
    C: Codec<Error = E> + Clone,
{
    type Message = C::Message;
    type Error = E;
    fn peer_endpoint(&self) -> std::result::Result<Endpoint, Self::Error> {
        Ok(self.peer_endpoint.clone())
    }

    fn local_endpoint(&self) -> std::result::Result<Endpoint, Self::Error> {
        Ok(self.local_endpoint.clone())
    }

    async fn recv(&self) -> std::result::Result<Self::Message, Self::Error> {
        self.read_stream.lock().await.recv().await
    }

    async fn send(&self, msg: Self::Message) -> std::result::Result<(), Self::Error> {
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
impl<C, E> ConnListener for TcpListener<C>
where
    C: Codec<Error = E> + Clone + 'static,
    E: From<std::io::Error>,
{
    type Message = C::Message;
    type Error = E;
    fn local_endpoint(&self) -> std::result::Result<Endpoint, Self::Error> {
        Ok(Endpoint::new_tcp_addr(self.inner.local_addr()?))
    }

    async fn accept(&self) -> std::result::Result<Conn<C::Message, C::Error>, Self::Error> {
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

impl<C, E> From<TcpListener<C>> for Box<dyn ConnListener<Message = C::Message, Error = E>>
where
    C: Clone + Codec<Error = E> + 'static,
    E: From<std::io::Error>,
{
    fn from(listener: TcpListener<C>) -> Self {
        Box::new(listener)
    }
}

impl<C, E> ToConn for TcpConn<C>
where
    C: Codec<Error = E> + Clone + 'static,
{
    type Message = C::Message;
    type Error = E;
    fn to_conn(self) -> Conn<Self::Message, Self::Error> {
        Box::new(self)
    }
}

impl<C, E> ToListener for TcpListener<C>
where
    C: Clone + Codec<Error = E> + 'static,
    E: From<std::io::Error>,
{
    type Message = C::Message;
    type Error = E;
    fn to_listener(self) -> Listener<Self::Message, Self::Error> {
        Box::new(self)
    }
}
