//! HTTP client backend: HTTP/1 (smol), HTTP/1+2 (tokio), and HTTP/3 (QUIC).

#[cfg(feature = "smol")]
mod h1;
#[cfg(feature = "tokio")]
mod h2;
#[cfg(feature = "http3")]
mod h3;

use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "tokio")]
use bytes::Bytes;
use http_body_util::BodyExt;
#[cfg(feature = "tokio")]
use http_body_util::Full;
use hyper::StatusCode;
use log::info;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

#[cfg(feature = "tokio")]
use hyper_util::client::legacy::Client as HyperClient;

#[cfg(feature = "tokio")]
use crate::hyper_exec::HyperExecutor;

use karyon_core::{
    async_util::{timeout, TaskGroup},
    util::random_32,
};

#[cfg(feature = "http3")]
use karyon_core::async_util::TaskResult;

use karyon_net::Endpoint;

use crate::{
    client::{Client, ClientBackend, ClientConfig, RequestID, WsCodec},
    codec::JsonRpcCodec,
    error::{Error, Result},
    message::{self, SubscriptionID},
};

#[cfg(feature = "http3")]
use crate::client::subscriptions::{Subscription, Subscriptions};

/// HTTP client backend for JSON-RPC.
pub(crate) struct HttpClientBackend {
    inner: HttpTransport,
    /// Shared with the Client so driver tasks (h1 per-request, h3
    /// connection) get cancelled when the Client stops.
    task_group: Arc<TaskGroup>,
}

// smol uses HTTP/1.1 only via `hyper::client::conn::http1`, which
// is the smallest hyper API that compiles against `smol-hyper`.
// tokio gets HTTP/1.1 + HTTP/2 with connection pooling for free
// through `hyper-util`'s legacy `Client`. The hyper client is boxed
// because it is much larger than the other variants.
enum HttpTransport {
    #[cfg(feature = "smol")]
    Smol { endpoint: Endpoint },
    #[cfg(feature = "tokio")]
    Tokio {
        endpoint: Endpoint,
        client: Box<HyperClient<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>>,
    },
    #[cfg(feature = "http3")]
    H3 { send_request: h3::H3SendRequest },
}

impl HttpClientBackend {
    /// Create an HTTP/1.1 + HTTP/2 client.
    pub(crate) fn new(endpoint: &Endpoint, task_group: Arc<TaskGroup>) -> Result<Self> {
        #[cfg(feature = "smol")]
        let inner = HttpTransport::Smol {
            endpoint: endpoint.clone(),
        };

        #[cfg(feature = "tokio")]
        let inner = HttpTransport::Tokio {
            endpoint: endpoint.clone(),
            client: Box::new(
                HyperClient::builder(HyperExecutor::new(task_group.clone())).build_http(),
            ),
        };

        Ok(Self { inner, task_group })
    }

    /// Create an HTTP/3 client (QUIC). The driver task is spawned via
    /// `task_group` so it gets cancelled when the Client stops.
    #[cfg(feature = "http3")]
    pub(crate) async fn new_h3(
        endpoint: &Endpoint,
        quic_config: &karyon_net::quic::ClientQuicConfig,
        task_group: Arc<TaskGroup>,
    ) -> Result<Self> {
        let send_request = h3::connect(endpoint, quic_config, task_group.clone()).await?;
        Ok(Self {
            inner: HttpTransport::H3 { send_request },
            task_group,
        })
    }

    /// Send a JSON-RPC request and return the response.
    pub(crate) async fn send_request(&self, msg: serde_json::Value) -> Result<message::Response> {
        let _ = &self.task_group;
        match &self.inner {
            #[cfg(feature = "smol")]
            HttpTransport::Smol { endpoint } => h1::send(endpoint, msg, &self.task_group).await,
            #[cfg(feature = "tokio")]
            HttpTransport::Tokio { endpoint, client } => h2::send(client, endpoint, msg).await,
            #[cfg(feature = "http3")]
            HttpTransport::H3 { send_request } => h3::send(send_request.clone(), msg).await,
        }
    }

    /// Subscribe over HTTP/3. Returns the initial response + recv stream.
    #[cfg(feature = "http3")]
    pub(crate) async fn subscribe_h3(
        &self,
        msg: serde_json::Value,
    ) -> Result<(message::Response, h3::H3RecvStream)> {
        let HttpTransport::H3 { send_request } = &self.inner else {
            return Err(Error::UnsupportedProtocol(
                "subscribe_h3 requires HTTP/3".into(),
            ));
        };
        h3::subscribe(send_request.clone(), msg).await
    }
}

/// Shared HTTP response parser (used by h1 and h2).
pub(super) async fn parse_response<B>(response: hyper::Response<B>) -> Result<message::Response>
where
    B: hyper::body::Body,
    B::Error: std::fmt::Display,
{
    if response.status() != StatusCode::OK {
        return Err(Error::HttpError(format!(
            "HTTP error: {}",
            response.status()
        )));
    }

    let body = response
        .collect()
        .await
        .map_err(|e| Error::HttpError(e.to_string()))?
        .to_bytes();

    let rpc_response: message::Response = serde_json::from_slice(&body)?;
    Ok(rpc_response)
}

/// Build the HTTP backend, optionally with HTTP/3 support.
pub(super) async fn build_backend(
    config: &ClientConfig,
    task_group: Arc<TaskGroup>,
) -> Result<ClientBackend> {
    #[cfg(feature = "http3")]
    let http_backend = match config.quic_config.as_ref() {
        Some(qc) => HttpClientBackend::new_h3(&config.endpoint, qc, task_group).await?,
        None => HttpClientBackend::new(&config.endpoint, task_group)?,
    };
    #[cfg(not(feature = "http3"))]
    let http_backend = HttpClientBackend::new(&config.endpoint, task_group)?;

    info!("HTTP client configured for endpoint: {}", config.endpoint);

    Ok(ClientBackend::Http {
        http_backend: Box::new(http_backend),
        #[cfg(feature = "http3")]
        subscriptions: Subscriptions::new(config.subscription_buffer_size),
    })
}

// -- Client impl (HTTP-specific) --

impl<B, W> Client<B, W>
where
    B: JsonRpcCodec,
    W: WsCodec,
{
    pub(super) async fn http_call<T: Serialize + DeserializeOwned>(
        &self,
        http_backend: &HttpClientBackend,
        method: &str,
        params: T,
    ) -> Result<message::Response> {
        let id: RequestID = random_32();
        let request = message::Request {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            id: json!(id),
            method: method.to_string(),
            params: Some(json!(params)),
        };
        let msg = serde_json::to_value(request)?;

        let response = match self.config.timeout {
            Some(t) => timeout(Duration::from_millis(t), http_backend.send_request(msg)).await?,
            None => http_backend.send_request(msg).await,
        }?;

        Ok(response)
    }

    /// Subscribe over HTTP/3.
    #[cfg(feature = "http3")]
    pub(super) async fn h3_subscribe<T: Serialize + DeserializeOwned>(
        self: &Arc<Self>,
        http_backend: &HttpClientBackend,
        method: &str,
        params: T,
    ) -> Result<Arc<Subscription>> {
        let ClientBackend::Http { subscriptions, .. } = &self.backend else {
            return Err(Error::InvalidState("not in HTTP mode".into()));
        };

        let id: RequestID = random_32();
        let request = message::Request {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            id: json!(id),
            method: method.to_string(),
            params: Some(json!(params)),
        };
        let msg = serde_json::to_value(request)?;

        let (response, recv_stream) = http_backend.subscribe_h3(msg).await?;

        if let Some(error) = response.error {
            return Err(Error::SubscribeError(error.code, error.message));
        }

        let sub_id = match response.result {
            Some(result) => serde_json::from_value::<SubscriptionID>(result)?,
            None => return Err(Error::InvalidMsg("Invalid subscription id".into())),
        };

        let sub = subscriptions.subscribe(sub_id).await;

        self.task_group.spawn(
            h3::notification_reader_task(recv_stream, sub.clone()),
            |res: TaskResult<Result<()>>| async move {
                log::debug!("H3 notification reader task ended: {res}");
            },
        );

        Ok(sub)
    }

    /// Unsubscribe over HTTP/3.
    #[cfg(feature = "http3")]
    pub(super) async fn h3_unsubscribe(
        &self,
        http_backend: &HttpClientBackend,
        method: &str,
        sub_id: SubscriptionID,
    ) -> Result<()> {
        let ClientBackend::Http { subscriptions, .. } = &self.backend else {
            return Err(Error::InvalidState("not in HTTP mode".into()));
        };

        let response = self.http_call(http_backend, method, sub_id).await?;
        if let Some(error) = response.error {
            return Err(Error::SubscribeError(error.code, error.message));
        }
        subscriptions.unsubscribe(&sub_id).await;
        Ok(())
    }
}
