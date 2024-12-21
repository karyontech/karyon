use std::{net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use futures_util::SinkExt;
use rustls_pki_types as pki_types;

use karyon_core::async_runtime::{
    io::{split, ReadHalf, WriteHalf},
    lock::Mutex,
    net::{TcpListener, TcpStream},
};

#[cfg(feature = "tls")]
use crate::async_rustls::{rustls, TlsAcceptor, TlsConnector, TlsStream};

use crate::{
    codec::Codec,
    connection::{Conn, Connection, ToConn},
    endpoint::Endpoint,
    listener::{ConnListener, Listener, ToListener},
    stream::{ReadStream, WriteStream},
    Result,
};

use super::tcp::TcpConfig;

/// TLS configuration
#[derive(Clone)]
pub struct ServerTlsConfig {
    pub tcp_config: TcpConfig,
    pub server_config: rustls::ServerConfig,
}

#[derive(Clone)]
pub struct ClientTlsConfig {
    pub tcp_config: TcpConfig,
    pub client_config: rustls::ClientConfig,
    pub dns_name: String,
}

/// TLS network connection implementation of the [`Connection`] trait.
pub struct TlsConn<C> {
    read_stream: Mutex<ReadStream<ReadHalf<TlsStream<TcpStream>>, C>>,
    write_stream: Mutex<WriteStream<WriteHalf<TlsStream<TcpStream>>, C>>,
    peer_endpoint: Endpoint,
    local_endpoint: Endpoint,
}

impl<C> TlsConn<C>
where
    C: Codec + Clone,
{
    /// Creates a new TlsConn
    pub fn new(
        conn: TlsStream<TcpStream>,
        codec: C,
        peer_endpoint: Endpoint,
        local_endpoint: Endpoint,
    ) -> Self {
        let (read, write) = split(conn);
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
impl<C, E> Connection for TlsConn<C>
where
    C: Clone + Codec<Error = E>,
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

/// Connects to the given TLS address and port.
pub async fn dial<C>(endpoint: &Endpoint, config: ClientTlsConfig, codec: C) -> Result<TlsConn<C>>
where
    C: Codec + Clone,
{
    let addr = SocketAddr::try_from(endpoint.clone())?;

    let connector = TlsConnector::from(Arc::new(config.client_config.clone()));

    let socket = TcpStream::connect(addr).await?;
    socket.set_nodelay(config.tcp_config.nodelay)?;

    let peer_endpoint = socket.peer_addr().map(Endpoint::new_tls_addr)?;
    let local_endpoint = socket.local_addr().map(Endpoint::new_tls_addr)?;

    let altname = pki_types::ServerName::try_from(config.dns_name.clone())?;
    let conn = connector.connect(altname, socket).await?;
    Ok(TlsConn::new(
        TlsStream::Client(conn),
        codec,
        peer_endpoint,
        local_endpoint,
    ))
}

/// Tls network listener implementation of the `Listener` [`ConnListener`] trait.
pub struct TlsListener<C> {
    inner: TcpListener,
    acceptor: TlsAcceptor,
    config: ServerTlsConfig,
    codec: C,
}

impl<C> TlsListener<C>
where
    C: Codec + Clone,
{
    pub fn new(
        acceptor: TlsAcceptor,
        listener: TcpListener,
        config: ServerTlsConfig,
        codec: C,
    ) -> Self {
        Self {
            inner: listener,
            acceptor,
            config: config.clone(),
            codec,
        }
    }
}

#[async_trait]
impl<C, E> ConnListener for TlsListener<C>
where
    C: Clone + Codec<Error = E> + 'static,
    E: From<std::io::Error>,
{
    type Message = C::Message;
    type Error = E;
    fn local_endpoint(&self) -> std::result::Result<Endpoint, Self::Error> {
        Ok(Endpoint::new_tls_addr(self.inner.local_addr()?))
    }

    async fn accept(&self) -> std::result::Result<Conn<C::Message, E>, Self::Error> {
        let (socket, _) = self.inner.accept().await?;
        socket.set_nodelay(self.config.tcp_config.nodelay)?;

        let peer_endpoint = socket.peer_addr().map(Endpoint::new_tls_addr)?;
        let local_endpoint = socket.local_addr().map(Endpoint::new_tls_addr)?;

        let conn = self.acceptor.accept(socket).await?;
        Ok(Box::new(TlsConn::new(
            TlsStream::Server(conn),
            self.codec.clone(),
            peer_endpoint,
            local_endpoint,
        )))
    }
}

/// Listens on the given TLS address and port.
pub async fn listen<C>(
    endpoint: &Endpoint,
    config: ServerTlsConfig,
    codec: C,
) -> Result<TlsListener<C>>
where
    C: Clone + Codec,
{
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let acceptor = TlsAcceptor::from(Arc::new(config.server_config.clone()));
    let listener = TcpListener::bind(addr).await?;
    Ok(TlsListener::new(acceptor, listener, config, codec))
}

impl<C, E> From<TlsListener<C>> for Listener<C::Message, E>
where
    C: Codec<Error = E> + Clone + 'static,
    E: From<std::io::Error>,
{
    fn from(listener: TlsListener<C>) -> Self {
        Box::new(listener)
    }
}

impl<C, E> ToConn for TlsConn<C>
where
    C: Codec<Error = E> + Clone + 'static,
{
    type Message = C::Message;
    type Error = E;
    fn to_conn(self) -> Conn<Self::Message, Self::Error> {
        Box::new(self)
    }
}

impl<C, E> ToListener for TlsListener<C>
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
