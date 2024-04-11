use std::{net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use rustls_pki_types as pki_types;

#[cfg(feature = "tokio")]
use async_tungstenite::tokio as async_tungstenite;

#[cfg(feature = "smol")]
use futures_rustls::{rustls, TlsAcceptor, TlsConnector};
#[cfg(feature = "tokio")]
use tokio_rustls::{rustls, TlsAcceptor, TlsConnector};

use karyon_core::async_runtime::{
    lock::Mutex,
    net::{TcpListener, TcpStream},
};

use crate::{
    codec::WebSocketCodec,
    connection::{Conn, Connection, ToConn},
    endpoint::Endpoint,
    listener::{ConnListener, Listener, ToListener},
    stream::WsStream,
    Result,
};

use super::tcp::TcpConfig;

/// WSS configuration
#[derive(Clone)]
pub struct ServerWssConfig {
    pub server_config: rustls::ServerConfig,
}

/// WSS configuration
#[derive(Clone)]
pub struct ClientWssConfig {
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
    // XXX: remove mutex
    inner: Mutex<WsStream<C>>,
    peer_endpoint: Endpoint,
    local_endpoint: Endpoint,
}

impl<C> WsConn<C>
where
    C: WebSocketCodec,
{
    /// Creates a new WsConn
    pub fn new(ws: WsStream<C>, peer_endpoint: Endpoint, local_endpoint: Endpoint) -> Self {
        Self {
            inner: Mutex::new(ws),
            peer_endpoint,
            local_endpoint,
        }
    }
}

#[async_trait]
impl<C> Connection for WsConn<C>
where
    C: WebSocketCodec,
{
    type Item = C::Item;
    fn peer_endpoint(&self) -> Result<Endpoint> {
        Ok(self.peer_endpoint.clone())
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(self.local_endpoint.clone())
    }

    async fn recv(&self) -> Result<Self::Item> {
        self.inner.lock().await.recv().await
    }

    async fn send(&self, msg: Self::Item) -> Result<()> {
        self.inner.lock().await.send(msg).await
    }
}

/// Ws network listener implementation of the `Listener` [`ConnListener`] trait.
pub struct WsListener<C> {
    inner: TcpListener,
    config: ServerWsConfig,
    codec: C,
    tls_acceptor: Option<TlsAcceptor>,
}

#[async_trait]
impl<C> ConnListener for WsListener<C>
where
    C: WebSocketCodec + Clone,
{
    type Item = C::Item;
    fn local_endpoint(&self) -> Result<Endpoint> {
        match self.config.wss_config {
            Some(_) => Ok(Endpoint::new_wss_addr(self.inner.local_addr()?)),
            None => Ok(Endpoint::new_ws_addr(self.inner.local_addr()?)),
        }
    }

    async fn accept(&self) -> Result<Conn<Self::Item>> {
        let (socket, _) = self.inner.accept().await?;
        socket.set_nodelay(self.config.tcp_config.nodelay)?;

        match &self.config.wss_config {
            Some(_) => match &self.tls_acceptor {
                Some(acceptor) => {
                    let peer_endpoint = socket.peer_addr().map(Endpoint::new_wss_addr)?;
                    let local_endpoint = socket.local_addr().map(Endpoint::new_wss_addr)?;

                    let tls_conn = acceptor.accept(socket).await?.into();
                    let conn = async_tungstenite::accept_async(tls_conn).await?;
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

                let conn = async_tungstenite::accept_async(socket).await?;

                Ok(Box::new(WsConn::new(
                    WsStream::new_ws(conn, self.codec.clone()),
                    peer_endpoint,
                    local_endpoint,
                )))
            }
        }
    }
}

/// Connects to the given WS address and port.
pub async fn dial<C>(endpoint: &Endpoint, config: ClientWsConfig, codec: C) -> Result<WsConn<C>>
where
    C: WebSocketCodec,
{
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let socket = TcpStream::connect(addr).await?;
    socket.set_nodelay(config.tcp_config.nodelay)?;

    match &config.wss_config {
        Some(conf) => {
            let peer_endpoint = socket.peer_addr().map(Endpoint::new_wss_addr)?;
            let local_endpoint = socket.local_addr().map(Endpoint::new_wss_addr)?;

            let connector = TlsConnector::from(Arc::new(conf.client_config.clone()));

            let altname = pki_types::ServerName::try_from(conf.dns_name.clone())?;
            let tls_conn = connector.connect(altname, socket).await?.into();
            let (conn, _resp) =
                async_tungstenite::client_async(endpoint.to_string(), tls_conn).await?;
            Ok(WsConn::new(
                WsStream::new_wss(conn, codec),
                peer_endpoint,
                local_endpoint,
            ))
        }
        None => {
            let peer_endpoint = socket.peer_addr().map(Endpoint::new_ws_addr)?;
            let local_endpoint = socket.local_addr().map(Endpoint::new_ws_addr)?;
            let (conn, _resp) =
                async_tungstenite::client_async(endpoint.to_string(), socket).await?;
            Ok(WsConn::new(
                WsStream::new_ws(conn, codec),
                peer_endpoint,
                local_endpoint,
            ))
        }
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
            tls_acceptor: None,
        }),
    }
}

impl<C> From<WsListener<C>> for Listener<C::Item>
where
    C: WebSocketCodec + Clone,
{
    fn from(listener: WsListener<C>) -> Self {
        Box::new(listener)
    }
}

impl<C> ToConn for WsConn<C>
where
    C: WebSocketCodec,
{
    type Item = C::Item;
    fn to_conn(self) -> Conn<Self::Item> {
        Box::new(self)
    }
}

impl<C> ToListener for WsListener<C>
where
    C: WebSocketCodec + Clone,
{
    type Item = C::Item;
    fn to_listener(self) -> Listener<Self::Item> {
        self.into()
    }
}
