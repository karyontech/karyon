use std::net::SocketAddr;

#[cfg(feature = "tls")]
use std::sync::Arc;

use async_trait::async_trait;
#[cfg(feature = "tls")]
use rustls_pki_types as pki_types;

use async_tungstenite::tungstenite::Error;

#[cfg(feature = "smol")]
use async_tungstenite::{accept_async, client_async};

#[cfg(feature = "tokio")]
use async_tungstenite::tokio::{accept_async, client_async};

use karyon_core::async_runtime::{
    lock::Mutex,
    net::{TcpListener, TcpStream},
};

#[cfg(feature = "tls")]
use crate::async_rustls::{rustls, TlsAcceptor, TlsConnector};

use crate::{
    codec::WebSocketCodec,
    connection::{Conn, Connection, ToConn},
    endpoint::Endpoint,
    listener::{ConnListener, Listener, ToListener},
    stream::{ReadWsStream, WriteWsStream, WsStream},
    Result,
};

use super::tcp::TcpConfig;

/// WSS configuration
#[derive(Clone)]
pub struct ServerWssConfig {
    #[cfg(feature = "tls")]
    pub server_config: rustls::ServerConfig,
}

/// WSS configuration
#[derive(Clone)]
pub struct ClientWssConfig {
    #[cfg(feature = "tls")]
    pub client_config: rustls::ClientConfig,
    pub dns_name: String,
}

/// WS configuration
#[derive(Clone, Default)]
pub struct ServerWsConfig {
    pub tcp_config: TcpConfig,
    pub wss_config: Option<ServerWssConfig>,
}

/// WS configuration
#[derive(Clone, Default)]
pub struct ClientWsConfig {
    pub tcp_config: TcpConfig,
    pub wss_config: Option<ClientWssConfig>,
}

/// WS network connection implementation of the [`Connection`] trait.
pub struct WsConn<C> {
    read_stream: Mutex<ReadWsStream<C>>,
    write_stream: Mutex<WriteWsStream<C>>,
    peer_endpoint: Endpoint,
    local_endpoint: Endpoint,
}

impl<C> WsConn<C>
where
    C: WebSocketCodec + Clone,
{
    /// Creates a new WsConn
    pub fn new(ws: WsStream<C>, peer_endpoint: Endpoint, local_endpoint: Endpoint) -> Self {
        let (read, write) = ws.split();
        Self {
            read_stream: Mutex::new(read),
            write_stream: Mutex::new(write),
            peer_endpoint,
            local_endpoint,
        }
    }
}

#[async_trait]
impl<C, E> Connection for WsConn<C>
where
    C: WebSocketCodec<Error = E>,
    E: From<Error>,
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

/// Ws network listener implementation of the `Listener` [`ConnListener`] trait.
pub struct WsListener<C> {
    inner: TcpListener,
    config: ServerWsConfig,
    codec: C,
    #[cfg(feature = "tls")]
    tls_acceptor: Option<TlsAcceptor>,
}

#[async_trait]
impl<C, E> ConnListener for WsListener<C>
where
    C: WebSocketCodec<Error = E> + Clone + 'static,
    E: From<Error> + From<std::io::Error>,
{
    type Message = C::Message;
    type Error = E;
    fn local_endpoint(&self) -> std::result::Result<Endpoint, Self::Error> {
        match self.config.wss_config {
            Some(_) => Ok(Endpoint::new_wss_addr(self.inner.local_addr()?)),
            None => Ok(Endpoint::new_ws_addr(self.inner.local_addr()?)),
        }
    }

    async fn accept(&self) -> std::result::Result<Conn<Self::Message, Self::Error>, Self::Error> {
        let (socket, _) = self.inner.accept().await?;
        socket.set_nodelay(self.config.tcp_config.nodelay)?;

        match &self.config.wss_config {
            #[cfg(feature = "tls")]
            Some(_) => match &self.tls_acceptor {
                Some(acceptor) => {
                    let peer_endpoint = socket.peer_addr().map(Endpoint::new_wss_addr)?;
                    let local_endpoint = socket.local_addr().map(Endpoint::new_wss_addr)?;

                    let tls_conn = acceptor.accept(socket).await?.into();
                    let conn = accept_async(tls_conn).await?;
                    Ok(Box::new(WsConn::new(
                        WsStream::new_wss(conn, self.codec.clone()),
                        peer_endpoint,
                        local_endpoint,
                    )))
                }
                None => unreachable!(),
            },
            None => {
                let peer_endpoint = socket.peer_addr().map(Endpoint::new_ws_addr)?;
                let local_endpoint = socket.local_addr().map(Endpoint::new_ws_addr)?;

                let conn = accept_async(socket).await?;

                Ok(Box::new(WsConn::new(
                    WsStream::new_ws(conn, self.codec.clone()),
                    peer_endpoint,
                    local_endpoint,
                )))
            }
            #[cfg(not(feature = "tls"))]
            _ => unreachable!(),
        }
    }
}

/// Connects to the given WS address and port.
pub async fn dial<C>(endpoint: &Endpoint, config: ClientWsConfig, codec: C) -> Result<WsConn<C>>
where
    C: WebSocketCodec + Clone,
{
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let socket = TcpStream::connect(addr).await?;
    socket.set_nodelay(config.tcp_config.nodelay)?;

    match &config.wss_config {
        #[cfg(feature = "tls")]
        Some(conf) => {
            let peer_endpoint = socket.peer_addr().map(Endpoint::new_wss_addr)?;
            let local_endpoint = socket.local_addr().map(Endpoint::new_wss_addr)?;

            let connector = TlsConnector::from(Arc::new(conf.client_config.clone()));

            let altname = pki_types::ServerName::try_from(conf.dns_name.clone())?;
            let tls_conn = connector.connect(altname, socket).await?.into();
            let (conn, _resp) = client_async(endpoint.to_string(), tls_conn)
                .await
                .map_err(Box::new)?;
            Ok(WsConn::new(
                WsStream::new_wss(conn, codec),
                peer_endpoint,
                local_endpoint,
            ))
        }
        None => {
            let peer_endpoint = socket.peer_addr().map(Endpoint::new_ws_addr)?;
            let local_endpoint = socket.local_addr().map(Endpoint::new_ws_addr)?;
            let (conn, _resp) = client_async(endpoint.to_string(), socket)
                .await
                .map_err(Box::new)?;
            Ok(WsConn::new(
                WsStream::new_ws(conn, codec),
                peer_endpoint,
                local_endpoint,
            ))
        }
        #[cfg(not(feature = "tls"))]
        _ => unreachable!(),
    }
}

/// Listens on the given WS address and port.
pub async fn listen<C>(
    endpoint: &Endpoint,
    config: ServerWsConfig,
    codec: C,
) -> Result<WsListener<C>> {
    let addr = SocketAddr::try_from(endpoint.clone())?;

    let listener = TcpListener::bind(addr).await?;
    match &config.wss_config {
        #[cfg(feature = "tls")]
        Some(conf) => {
            let acceptor = TlsAcceptor::from(Arc::new(conf.server_config.clone()));
            Ok(WsListener {
                inner: listener,
                config,
                codec,
                tls_acceptor: Some(acceptor),
            })
        }
        None => Ok(WsListener {
            inner: listener,
            config,
            codec,
            #[cfg(feature = "tls")]
            tls_acceptor: None,
        }),
        #[cfg(not(feature = "tls"))]
        _ => unreachable!(),
    }
}

impl<C, E> From<WsListener<C>> for Listener<C::Message, E>
where
    C: WebSocketCodec<Error = E> + Clone + 'static,
    E: From<Error> + From<std::io::Error>,
{
    fn from(listener: WsListener<C>) -> Self {
        Box::new(listener)
    }
}

impl<C, E> ToConn for WsConn<C>
where
    C: WebSocketCodec<Error = E> + 'static,
    E: From<Error>,
{
    type Message = C::Message;
    type Error = E;
    fn to_conn(self) -> Conn<Self::Message, Self::Error> {
        Box::new(self)
    }
}

impl<C, E> ToListener for WsListener<C>
where
    C: WebSocketCodec<Error = E> + Clone + 'static,
    E: From<Error> + From<std::io::Error>,
{
    type Message = C::Message;
    type Error = E;
    fn to_listener(self) -> Listener<Self::Message, Self::Error> {
        Box::new(self)
    }
}
