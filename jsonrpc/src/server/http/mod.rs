//! HTTP server: HTTP/1.1 + HTTP/2 over TCP, optional HTTP/3 over QUIC.

mod h1h2;
#[cfg(feature = "http3")]
mod h3;

use std::{net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};
use serde_json::to_value;

use karyon_core::async_runtime::net::TcpListener;

#[cfg(feature = "http3")]
use karyon_core::async_util::select;

use karyon_net::Endpoint;

pub(super) use crate::hyper_exec::HyperExecutor;

use crate::{
    codec::{encode_msg, JsonRpcCodec},
    error::{Error, Result},
    message,
    server::{Server, FAILED_TO_PARSE_ERROR_MSG},
};

/// Max HTTP request body size (1 MB).
pub(super) const MAX_HTTP_BODY_SIZE: u64 = 1024 * 1024;

// Pre-encoded JSON-RPC errors returned before a message can be parsed.
pub(super) const ERR_METHOD_NOT_ALLOWED: &str = r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Only POST method is accepted"},"id":null}"#;
pub(super) const ERR_BODY_TOO_LARGE: &str =
    r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Request body too large"},"id":null}"#;
pub(super) const ERR_READ_BODY: &str = r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Failed to read request body"},"id":null}"#;
pub(super) const ERR_ENCODE_RESPONSE: &str =
    r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Failed to encode response"},"id":null}"#;

/// Runs an HTTP server. Hides the codec type, like `AsyncAcceptor`
/// does for the stream transports.
#[async_trait]
pub(crate) trait HttpAcceptor: Send + Sync {
    /// Run the accept loop.
    async fn accept_loop(&self, server: Arc<Server>) -> Result<()>;
    fn local_endpoint(&self) -> Result<Endpoint>;
}

/// HTTP server (TCP for h1/h2, optional QUIC for h3).
pub(crate) struct HttpServer<C> {
    tcp_listener: TcpListener,
    /// Encodes and decodes all HTTP bodies and HTTP/3 streams.
    codec: C,
    #[cfg(feature = "http3")]
    quic_endpoint: Option<karyon_net::quic::QuicEndpoint>,
}

impl<C: JsonRpcCodec> HttpServer<C> {
    /// Create HTTP/1.1 + HTTP/2 server.
    pub(crate) async fn new(endpoint: &Endpoint, codec: C) -> Result<Self> {
        let addr = SocketAddr::try_from(endpoint.clone())?;
        let tcp_listener = TcpListener::bind(addr).await?;
        Ok(Self {
            tcp_listener,
            codec,
            #[cfg(feature = "http3")]
            quic_endpoint: None,
        })
    }

    /// Create HTTP/1.1 + HTTP/2 + HTTP/3 server.
    #[cfg(feature = "http3")]
    pub(crate) async fn new_h3(
        endpoint: &Endpoint,
        quic_config: karyon_net::quic::ServerQuicConfig,
        codec: C,
    ) -> Result<Self> {
        let addr = SocketAddr::try_from(endpoint.clone())?;
        let tcp_listener = TcpListener::bind(addr).await?;
        let actual_addr = tcp_listener.local_addr()?;
        let quic_ep_addr = Endpoint::new_quic_addr(actual_addr);
        let quic_endpoint =
            karyon_net::quic::QuicEndpoint::listen(&quic_ep_addr, quic_config).await?;
        Ok(Self {
            tcp_listener,
            codec,
            quic_endpoint: Some(quic_endpoint),
        })
    }
}

#[async_trait]
impl<C: JsonRpcCodec> HttpAcceptor for HttpServer<C> {
    async fn accept_loop(&self, server: Arc<Server>) -> Result<()> {
        let codec = &self.codec;

        #[cfg(feature = "http3")]
        if let Some(ref quic_ep) = self.quic_endpoint {
            loop {
                select(
                    h1h2::accept_tcp(&server, &self.tcp_listener, codec),
                    h3::accept_h3(&server, quic_ep, codec),
                )
                .await;
            }
        }

        loop {
            h1h2::accept_tcp(&server, &self.tcp_listener, codec).await;
        }
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        let addr = self.tcp_listener.local_addr()?;
        format!("http://{addr}/")
            .parse()
            .map_err(|e: karyon_net::Error| Error::HttpError(e.to_string()))
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

pub(super) fn json_response_bytes(status: StatusCode, body: Bytes) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(body))
        .unwrap()
}

/// Encode a JSON-RPC response with the server codec.
pub(super) fn encode_response<C: JsonRpcCodec>(
    codec: &C,
    resp: &message::Response,
) -> Result<Bytes> {
    encode_msg(codec, &to_value(resp)?)
}

/// Build the JSON-RPC parse error response.
pub(super) fn parse_error_response() -> message::Response {
    message::Response {
        error: Some(message::Error {
            code: message::PARSE_ERROR_CODE,
            message: FAILED_TO_PARSE_ERROR_MSG.to_string(),
            data: None,
        }),
        ..Default::default()
    }
}
