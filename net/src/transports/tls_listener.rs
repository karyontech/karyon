use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use karyon_core::async_util::timeout;

use crate::{
    Addr, ByteStream, Endpoint, Result, ServerLayer,
    tls::{ServerTlsConfig, TlsLayer},
};

use super::{ACCEPT_HANDSHAKE_TIMEOUT, tcp::TcpListener};

/// TLS listener. Wraps a TCP listener with TLS acceptance.
pub struct TlsListener {
    inner: TcpListener,
    layer: TlsLayer,
    /// Max time `accept` waits for a handshake.
    handshake_timeout: Duration,
}

impl TlsListener {
    /// Create a TLS listener from a TCP listener and TLS config.
    pub fn new(inner: TcpListener, config: ServerTlsConfig) -> Self {
        Self {
            inner,
            layer: TlsLayer::server(config),
            handshake_timeout: ACCEPT_HANDSHAKE_TIMEOUT,
        }
    }

    /// Set the max time `accept` waits for a handshake. Defaults to 10s.
    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// Accept a new TLS connection. The handshake runs inline and fails
    /// after the handshake timeout (10s by default), so a slow client
    /// stalls a plain accept loop for up to that long. Servers should accept on a `TcpListener` and run
    /// `TlsLayer::handshake` in a separate task instead.
    pub async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        let stream = self.inner.accept().await?;
        timeout(
            self.handshake_timeout,
            ServerLayer::handshake(&self.layer, stream),
        )
        .await?
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
