use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use crate::async_rustls::rustls;

use karyon_core::async_runtime::io::{AsyncRead, AsyncWrite};

#[cfg(feature = "tokio")]
use tokio::io::{AsyncWrite as TokioAsyncWrite, ReadBuf};

#[cfg(feature = "smol")]
use futures_util::io::{AsyncRead as FutAsyncRead, AsyncWrite as FutAsyncWrite};

use crate::{stream::ByteStream, stream_mux::StreamMux, Bytes, Endpoint, Error, Result};

/// Default read chunk size for QUIC streams.
const DEFAULT_READ_CHUNK_SIZE: usize = 1024 * 1024; // 1MB

/// A QUIC send stream. Re-exported from quinn for use by higher layers.
pub type QuicSendStream = quinn::SendStream;

/// A QUIC receive stream. Re-exported from quinn for use by higher layers.
pub type QuicRecvStream = quinn::RecvStream;

/// QUIC configuration.
#[derive(Clone)]
pub struct QuicConfig {
    /// Maximum concurrent bidirectional streams.
    pub max_bi_streams: u64,
    /// Maximum concurrent unidirectional streams.
    pub max_uni_streams: u64,
    /// Keep-alive interval. None to disable.
    pub keep_alive_interval: Option<Duration>,
    /// Idle timeout.
    pub idle_timeout: Option<Duration>,
    /// Enable datagrams.
    pub enable_datagrams: bool,
    /// Read chunk size for stream reads (bytes).
    pub read_chunk_size: usize,
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self {
            max_bi_streams: 100,
            max_uni_streams: 100,
            keep_alive_interval: Some(Duration::from_secs(5)),
            idle_timeout: Some(Duration::from_secs(30)),
            enable_datagrams: false,
            read_chunk_size: DEFAULT_READ_CHUNK_SIZE,
        }
    }
}

/// Server-side QUIC configuration. Build from either a cert chain +
/// private key (`new`) or a pre-built rustls config (`from_rustls`).
#[derive(Clone)]
pub struct ServerQuicConfig {
    source: ServerSource,
    config: QuicConfig,
}

#[derive(Clone)]
enum ServerSource {
    Certs {
        cert_chain: Vec<CertificateDer<'static>>,
        private_key: Arc<PrivateKeyDer<'static>>,
    },
    Rustls(rustls::ServerConfig),
}

impl ServerQuicConfig {
    /// Create a config from a cert chain + private key.
    pub fn new(
        cert_chain: Vec<CertificateDer<'static>>,
        private_key: Arc<PrivateKeyDer<'static>>,
    ) -> Self {
        Self {
            source: ServerSource::Certs {
                cert_chain,
                private_key,
            },
            config: QuicConfig::default(),
        }
    }

    /// Create a config from a pre-built rustls `ServerConfig` (for
    /// custom verifiers, client-auth, etc).
    pub fn from_rustls(rustls_config: rustls::ServerConfig) -> Self {
        Self {
            source: ServerSource::Rustls(rustls_config),
            config: QuicConfig::default(),
        }
    }

    /// Override the QUIC transport parameters.
    pub fn with_config(mut self, config: QuicConfig) -> Self {
        self.config = config;
        self
    }

    pub(crate) fn build(self) -> Result<quinn::ServerConfig> {
        let mut server_config = match self.source {
            ServerSource::Certs {
                cert_chain,
                private_key,
            } => quinn::ServerConfig::with_single_cert(cert_chain, private_key.clone_key())
                .map_err(|e| Error::TlsConfig(e.to_string()))?,
            ServerSource::Rustls(rustls_config) => {
                let quic_config = quinn::crypto::rustls::QuicServerConfig::try_from(rustls_config)
                    .map_err(|e| Error::QuicConfigError(e.to_string()))?;
                quinn::ServerConfig::with_crypto(Arc::new(quic_config))
            }
        };
        server_config.transport_config(Arc::new(build_transport_config(&self.config)));
        Ok(server_config)
    }
}

/// Client-side QUIC configuration. Build from either a root cert list
/// (`new`) or a pre-built rustls config (`from_rustls`).
#[derive(Clone)]
pub struct ClientQuicConfig {
    source: ClientSource,
    server_name: String,
    config: QuicConfig,
}

#[derive(Clone)]
enum ClientSource {
    Roots(Vec<CertificateDer<'static>>),
    Rustls(Box<rustls::ClientConfig>),
}

impl ClientQuicConfig {
    /// Create a config from a list of trusted root certs + server name.
    pub fn new(root_certs: Vec<CertificateDer<'static>>, server_name: impl Into<String>) -> Self {
        Self {
            source: ClientSource::Roots(root_certs),
            server_name: server_name.into(),
            config: QuicConfig::default(),
        }
    }

    /// Create a config from a pre-built rustls `ClientConfig` + server name
    /// (for custom verifiers, client-auth certs, etc).
    pub fn from_rustls(
        rustls_config: rustls::ClientConfig,
        server_name: impl Into<String>,
    ) -> Self {
        Self {
            source: ClientSource::Rustls(Box::new(rustls_config)),
            server_name: server_name.into(),
            config: QuicConfig::default(),
        }
    }

    /// Override the QUIC transport parameters.
    pub fn with_config(mut self, config: QuicConfig) -> Self {
        self.config = config;
        self
    }

    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    pub(crate) fn build(self) -> Result<quinn::ClientConfig> {
        let mut client_config = match self.source {
            ClientSource::Roots(root_certs) => {
                let mut root_store = rustls::RootCertStore::empty();
                for cert in root_certs {
                    root_store
                        .add(cert)
                        .map_err(|e| Error::TlsConfig(e.to_string()))?;
                }
                quinn::ClientConfig::with_root_certificates(Arc::new(root_store))
                    .map_err(|e| Error::TlsConfig(e.to_string()))?
            }
            ClientSource::Rustls(rustls_config) => {
                let quic_config = quinn::crypto::rustls::QuicClientConfig::try_from(*rustls_config)
                    .map_err(|e| Error::QuicConfigError(e.to_string()))?;
                quinn::ClientConfig::new(Arc::new(quic_config))
            }
        };
        client_config.transport_config(Arc::new(build_transport_config(&self.config)));
        Ok(client_config)
    }
}

fn build_transport_config(config: &QuicConfig) -> quinn::TransportConfig {
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(
        quinn::VarInt::from_u64(config.max_bi_streams).unwrap_or(quinn::VarInt::from_u32(100u32)),
    );
    transport.max_concurrent_uni_streams(
        quinn::VarInt::from_u64(config.max_uni_streams).unwrap_or(quinn::VarInt::from_u32(100u32)),
    );
    if let Some(interval) = config.keep_alive_interval {
        transport.keep_alive_interval(Some(interval));
    }
    if let Some(timeout) = config.idle_timeout {
        if let Ok(idle) = quinn::IdleTimeout::try_from(timeout) {
            transport.max_idle_timeout(Some(idle));
        }
    }
    // Disable datagrams explicitly when the caller opts out.
    if !config.enable_datagrams {
        transport.datagram_receive_buffer_size(None);
    }
    transport
}

/// A QUIC endpoint that can listen for and initiate connections.
pub struct QuicEndpoint {
    inner: quinn::Endpoint,
    local_endpoint: Endpoint,
}

impl QuicEndpoint {
    /// Bind to a local address and start listening with the given server config.
    pub async fn listen(endpoint: &Endpoint, config: ServerQuicConfig) -> Result<Self> {
        let addr = SocketAddr::try_from(endpoint.clone())?;
        let server_config = config.build()?;
        let inner = quinn::Endpoint::server(server_config, addr)?;
        let local_addr = inner.local_addr()?;
        let local_endpoint = Endpoint::new_quic_addr(local_addr);
        Ok(Self {
            inner,
            local_endpoint,
        })
    }

    /// Connect to a remote QUIC endpoint.
    pub async fn dial(endpoint: &Endpoint, config: ClientQuicConfig) -> Result<QuicConn> {
        let addr = SocketAddr::try_from(endpoint.clone())?;
        let server_name = config.server_name.clone();
        let client_config = config.build()?;
        // Bind the client socket in the same address family as the target.
        let bind_addr: SocketAddr = if addr.is_ipv6() {
            "[::]:0".parse().unwrap()
        } else {
            "0.0.0.0:0".parse().unwrap()
        };
        let mut quinn_endpoint = quinn::Endpoint::client(bind_addr)?;
        quinn_endpoint.set_default_client_config(client_config);

        let connection = quinn_endpoint.connect(addr, &server_name)?.await?;
        let peer_endpoint = Endpoint::new_quic_addr(connection.remote_address());
        let local_addr = quinn_endpoint.local_addr()?;
        let local_endpoint = Endpoint::new_quic_addr(local_addr);

        Ok(QuicConn {
            inner: connection,
            peer_endpoint,
            local_endpoint,
        })
    }

    /// Accept an incoming QUIC connection.
    pub async fn accept(&self) -> Result<QuicConn> {
        let incoming = self.inner.accept().await.ok_or(Error::ConnectionClosed)?;

        let connection = incoming.await?;
        let peer_endpoint = Endpoint::new_quic_addr(connection.remote_address());
        let local_endpoint = self.local_endpoint.clone();

        Ok(QuicConn {
            inner: connection,
            peer_endpoint,
            local_endpoint,
        })
    }

    /// Returns the local endpoint.
    pub fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(self.local_endpoint.clone())
    }

    /// Close the endpoint.
    pub fn close(&self, code: u32, reason: &[u8]) {
        self.inner.close(quinn::VarInt::from_u32(code), reason);
    }
}

/// A QUIC connection. Manages streams and datagrams.
/// This is NOT a single read/write channel — it is a stream factory.
pub struct QuicConn {
    inner: quinn::Connection,
    peer_endpoint: Endpoint,
    local_endpoint: Endpoint,
}

impl QuicConn {
    /// Open a new bidirectional stream.
    pub async fn open_bi(&self) -> Result<(QuicSendStream, QuicRecvStream)> {
        let (send, recv) = self.inner.open_bi().await?;
        Ok((send, recv))
    }

    /// Open a new unidirectional (send-only) stream.
    pub async fn open_uni(&self) -> Result<QuicSendStream> {
        let send = self.inner.open_uni().await?;
        Ok(send)
    }

    /// Accept a bidirectional stream opened by the peer.
    pub async fn accept_bi(&self) -> Result<(QuicSendStream, QuicRecvStream)> {
        let (send, recv) = self.inner.accept_bi().await?;
        Ok((send, recv))
    }

    /// Accept a unidirectional (receive-only) stream from the peer.
    pub async fn accept_uni(&self) -> Result<QuicRecvStream> {
        let recv = self.inner.accept_uni().await?;
        Ok(recv)
    }

    /// Send an unreliable datagram over the connection. Zero-copy —
    /// ownership of the `Bytes` allocation is passed to quinn.
    pub fn send_datagram(&self, data: Bytes) -> Result<()> {
        self.inner
            .send_datagram(data.into_inner())
            .map_err(|e| Error::QuicConfigError(e.to_string()))?;
        Ok(())
    }

    /// Receive an unreliable datagram. Zero-copy — wraps the allocation
    /// returned by quinn.
    pub async fn recv_datagram(&self) -> Result<Bytes> {
        let data = self.inner.read_datagram().await?;
        Ok(Bytes::from_inner(data))
    }

    /// Maximum datagram size the peer supports, or None if unsupported.
    pub fn max_datagram_size(&self) -> Option<usize> {
        self.inner.max_datagram_size()
    }

    /// Remote peer's address.
    pub fn peer_endpoint(&self) -> Result<Endpoint> {
        Ok(self.peer_endpoint.clone())
    }

    /// Local address.
    pub fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(self.local_endpoint.clone())
    }

    /// Current round-trip time estimate.
    pub fn rtt(&self) -> Duration {
        self.inner.rtt()
    }

    /// Close the connection gracefully.
    pub fn close(&self, code: u32, reason: &[u8]) {
        self.inner.close(quinn::VarInt::from_u32(code), reason);
    }

    /// Wait for the connection to be closed (by us or the peer).
    pub async fn closed(&self) -> quinn::ConnectionError {
        self.inner.closed().await
    }

    /// Returns a reference to the inner quinn connection.
    pub fn inner(&self) -> &quinn::Connection {
        &self.inner
    }

    /// Peer certificate chain from the QUIC TLS handshake. Quinn returns
    /// a type-erased `Box<dyn Any>`; for rustls-based QUIC (which is what
    /// karyon uses) it downcasts to `Vec<CertificateDer<'static>>`.
    pub fn peer_certificates(&self) -> Option<Vec<CertificateDer<'static>>> {
        let any = self.inner.peer_identity()?;
        any.downcast::<Vec<CertificateDer<'static>>>()
            .ok()
            .map(|b| *b)
    }
}

// -- StreamMux impl --

impl StreamMux for QuicConn {
    async fn open_stream(&self) -> Result<Box<dyn ByteStream>> {
        let (send, recv) = self.open_bi().await?;
        Ok(Box::new(QuicBiStream {
            send,
            recv,
            peer_endpoint: self.peer_endpoint.clone(),
            local_endpoint: self.local_endpoint.clone(),
        }))
    }

    async fn accept_stream(&self) -> Result<Box<dyn ByteStream>> {
        let (send, recv) = self.accept_bi().await?;
        Ok(Box::new(QuicBiStream {
            send,
            recv,
            peer_endpoint: self.peer_endpoint.clone(),
            local_endpoint: self.local_endpoint.clone(),
        }))
    }

    fn peer_endpoint(&self) -> Option<Endpoint> {
        Some(self.peer_endpoint.clone())
    }

    fn local_endpoint(&self) -> Option<Endpoint> {
        Some(self.local_endpoint.clone())
    }
}

/// Single QUIC bidirectional stream as a ByteStream.
pub struct QuicBiStream {
    send: QuicSendStream,
    recv: QuicRecvStream,
    peer_endpoint: Endpoint,
    local_endpoint: Endpoint,
}

impl ByteStream for QuicBiStream {
    fn peer_endpoint(&self) -> Option<Endpoint> {
        Some(self.peer_endpoint.clone())
    }
    fn local_endpoint(&self) -> Option<Endpoint> {
        Some(self.local_endpoint.clone())
    }
}

// Quinn streams use tokio IO traits natively.
// For smol builds, delegate manually via poll methods.

#[cfg(feature = "tokio")]
impl AsyncRead for QuicBiStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

#[cfg(feature = "smol")]
impl AsyncRead for QuicBiStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        FutAsyncRead::poll_read(Pin::new(&mut self.recv), cx, buf)
    }
}

#[cfg(feature = "tokio")]
impl AsyncWrite for QuicBiStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        TokioAsyncWrite::poll_write(Pin::new(&mut self.send), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        TokioAsyncWrite::poll_flush(Pin::new(&mut self.send), cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        TokioAsyncWrite::poll_shutdown(Pin::new(&mut self.send), cx)
    }
}

#[cfg(feature = "smol")]
impl AsyncWrite for QuicBiStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        FutAsyncWrite::poll_write(Pin::new(&mut self.send), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        FutAsyncWrite::poll_flush(Pin::new(&mut self.send), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        FutAsyncWrite::poll_close(Pin::new(&mut self.send), cx)
    }
}
