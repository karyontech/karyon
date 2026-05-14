//! HTTP server: HTTP/1.1 + HTTP/2 over TCP, optional HTTP/3 over QUIC.

mod h1h2;
#[cfg(feature = "http3")]
mod h3;

use std::{net::SocketAddr, sync::Arc};

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

use karyon_core::async_runtime::net::TcpListener;

#[cfg(feature = "http3")]
use karyon_core::async_util::select;

use karyon_net::Endpoint;

pub(super) use crate::hyper_exec::HyperExecutor;

use crate::{
    error::{Error, Result},
    server::Server,
};

/// Max HTTP request body size (1 MB).
pub(super) const MAX_HTTP_BODY_SIZE: u64 = 1024 * 1024;

// Pre-encoded JSON-RPC errors returned before a message can be parsed.
pub(super) const ERR_METHOD_NOT_ALLOWED: &str = r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Only POST method is accepted"},"id":null}"#;
pub(super) const ERR_BODY_TOO_LARGE: &str =
    r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Request body too large"},"id":null}"#;
pub(super) const ERR_READ_BODY: &str = r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Failed to read request body"},"id":null}"#;

/// HTTP server (TCP for h1/h2, optional QUIC for h3).
pub(crate) struct HttpServer {
    tcp_listener: TcpListener,
    #[cfg(feature = "http3")]
    quic_endpoint: Option<karyon_net::quic::QuicEndpoint>,
}

impl HttpServer {
    /// Create HTTP/1.1 + HTTP/2 server.
    pub(crate) async fn new(endpoint: &Endpoint) -> Result<Self> {
        let addr = SocketAddr::try_from(endpoint.clone())?;
        let tcp_listener = TcpListener::bind(addr).await?;
        Ok(Self {
            tcp_listener,
            #[cfg(feature = "http3")]
            quic_endpoint: None,
        })
    }

    /// Create HTTP/1.1 + HTTP/2 + HTTP/3 server.
    #[cfg(feature = "http3")]
    pub(crate) async fn new_h3(
        endpoint: &Endpoint,
        quic_config: karyon_net::quic::ServerQuicConfig,
    ) -> Result<Self> {
        let addr = SocketAddr::try_from(endpoint.clone())?;
        let tcp_listener = TcpListener::bind(addr).await?;
        let actual_addr = tcp_listener.local_addr()?;
        let quic_ep_addr = Endpoint::new_quic_addr(actual_addr);
        let quic_endpoint =
            karyon_net::quic::QuicEndpoint::listen(&quic_ep_addr, quic_config).await?;
        Ok(Self {
            tcp_listener,
            quic_endpoint: Some(quic_endpoint),
        })
    }

    pub(crate) fn local_endpoint(&self) -> Result<Endpoint> {
        let addr = self.tcp_listener.local_addr()?;
        format!("http://{addr}/")
            .parse()
            .map_err(|e: karyon_net::Error| Error::HttpError(e.to_string()))
    }
}

/// Run the HTTP accept loop.
pub(crate) async fn accept_loop(server: Arc<Server>, http_server: &HttpServer) -> Result<()> {
    #[cfg(feature = "http3")]
    if let Some(ref quic_ep) = http_server.quic_endpoint {
        loop {
            select(
                h1h2::accept_tcp(&server, &http_server.tcp_listener),
                h3::accept_h3(&server, quic_ep),
            )
            .await;
        }
    }

    loop {
        h1h2::accept_tcp(&server, &http_server.tcp_listener).await;
    }
}

// -- Shared helpers --

pub(super) fn json_response(status: StatusCode, body: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

pub(super) fn json_response_bytes(status: StatusCode, body: Vec<u8>) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}
