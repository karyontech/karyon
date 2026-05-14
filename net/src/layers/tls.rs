use std::{
    io,
    net::{IpAddr, Ipv4Addr},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use rustls_pki_types::{self as pki_types, CertificateDer};

use karyon_core::async_runtime::io::{AsyncRead, AsyncWrite};

#[cfg(feature = "tokio")]
use tokio::io::ReadBuf;

use crate::{
    async_rustls::{rustls, TlsAcceptor, TlsConnector, TlsStream},
    layer::{ClientLayer, ServerLayer},
    transports::tcp::TcpListener,
    Addr, ByteStream, Endpoint, Error, Result,
};

/// TLS client config.
#[derive(Clone)]
pub struct ClientTlsConfig {
    pub client_config: rustls::ClientConfig,
    pub dns_name: String,
}

/// TLS server config.
#[derive(Clone)]
pub struct ServerTlsConfig {
    pub server_config: rustls::ServerConfig,
}

/// TLS middleware layer. Implements both `ClientLayer` and `ServerLayer`.
///
/// Wraps any `ByteStream` with TLS encryption.
///
/// # Example
///
/// ```no_run
/// use karyon_net::{tcp, ClientLayer, Endpoint};
/// use karyon_net::tls::{TlsLayer, ClientTlsConfig};
///
/// async {
///     let ep: Endpoint = "tcp://127.0.0.1:443".parse().unwrap();
///     let stream = tcp::connect(&ep, Default::default()).await.unwrap();
///     // let tls_stream = ClientLayer::handshake(
///     //     &TlsLayer::client(config), stream
///     // ).await.unwrap();
/// };
/// ```
#[derive(Clone)]
pub struct TlsLayer {
    client_config: Option<ClientTlsConfig>,
    server_config: Option<ServerTlsConfig>,
}

impl TlsLayer {
    /// Create a TLS layer for client connections.
    pub fn client(config: ClientTlsConfig) -> Self {
        Self {
            client_config: Some(config),
            server_config: None,
        }
    }

    /// Create a TLS layer for server connections.
    pub fn server(config: ServerTlsConfig) -> Self {
        Self {
            client_config: None,
            server_config: Some(config),
        }
    }
}

impl ClientLayer<Box<dyn ByteStream>, Box<dyn ByteStream>> for TlsLayer {
    async fn handshake(&self, stream: Box<dyn ByteStream>) -> Result<Box<dyn ByteStream>> {
        let config = self.client_config.as_ref().ok_or_else(|| {
            Error::IO(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing TLS client config",
            ))
        })?;

        let connector = TlsConnector::from(Arc::new(config.client_config.clone()));
        let dns = pki_types::ServerName::try_from(config.dns_name.clone())?;

        let peer = stream.peer_endpoint();
        let local = stream.local_endpoint();
        let tls = connector.connect(dns, stream).await?;

        Ok(Box::new(TlsByteStream {
            inner: TlsStream::Client(tls),
            peer_endpoint: peer,
            local_endpoint: local,
        }))
    }
}

impl ServerLayer<Box<dyn ByteStream>, Box<dyn ByteStream>> for TlsLayer {
    async fn handshake(&self, stream: Box<dyn ByteStream>) -> Result<Box<dyn ByteStream>> {
        let config = self.server_config.as_ref().ok_or_else(|| {
            Error::IO(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing TLS server config",
            ))
        })?;

        let acceptor = TlsAcceptor::from(Arc::new(config.server_config.clone()));

        let peer = stream.peer_endpoint();
        let local = stream.local_endpoint();
        let tls = acceptor.accept(stream).await?;

        Ok(Box::new(TlsByteStream {
            inner: TlsStream::Server(tls),
            peer_endpoint: peer,
            local_endpoint: local,
        }))
    }
}

/// TLS stream wrapping any `ByteStream`.
pub struct TlsByteStream {
    inner: TlsStream<Box<dyn ByteStream>>,
    peer_endpoint: Option<Endpoint>,
    local_endpoint: Option<Endpoint>,
}

impl ByteStream for TlsByteStream {
    fn peer_endpoint(&self) -> Option<Endpoint> {
        self.peer_endpoint.clone()
    }
    fn local_endpoint(&self) -> Option<Endpoint> {
        self.local_endpoint.clone()
    }
    fn peer_certificates(&self) -> Option<Vec<CertificateDer<'static>>> {
        // TlsStream::get_ref() exposes the rustls connection state;
        // peer_certificates() returns whatever the peer presented during
        // the handshake (if any).
        let (_, state) = self.inner.get_ref();
        state
            .peer_certificates()
            .map(|certs| certs.iter().map(|c| c.clone().into_owned()).collect())
    }
}

// -- AsyncRead / AsyncWrite delegation --

#[cfg(feature = "smol")]
impl AsyncRead for TlsByteStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

#[cfg(feature = "tokio")]
impl AsyncRead for TlsByteStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

#[cfg(feature = "smol")]
impl AsyncWrite for TlsByteStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

#[cfg(feature = "tokio")]
impl AsyncWrite for TlsByteStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// TLS listener. Wraps a TCP listener with TLS acceptance.
pub struct TlsListener {
    inner: TcpListener,
    layer: TlsLayer,
}

impl TlsListener {
    /// Create a TLS listener from a TCP listener and TLS config.
    pub fn new(inner: TcpListener, config: ServerTlsConfig) -> Self {
        Self {
            inner,
            layer: TlsLayer::server(config),
        }
    }

    /// Accept a new TLS connection.
    pub async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        let stream = self.inner.accept().await?;
        ServerLayer::handshake(&self.layer, stream).await
    }

    /// Local endpoint this listener is bound to.
    pub fn local_endpoint(&self) -> Result<Endpoint> {
        let tcp_ep = self.inner.local_endpoint()?;
        let port = tcp_ep.port().unwrap_or(0);
        let addr = tcp_ep
            .addr()
            .unwrap_or(Addr::Ip(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
        Ok(Endpoint::Tls(addr, port))
    }
}
