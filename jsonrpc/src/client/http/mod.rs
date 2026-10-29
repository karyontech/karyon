//! HTTP client backend: HTTP/1.1 (smol, or pooled on tokio), opt-in
//! HTTP/2 over plain TCP, and HTTP/3 (QUIC).

mod h1;
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
use hyper_util::client::legacy::{connect::HttpConnector, Client as HyperClient};

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
    codec::{decode_body, JsonRpcCodec},
    error::{Error, Result},
    message::{self, SubscriptionID},
};

#[cfg(feature = "http3")]
use std::collections::HashMap;

#[cfg(feature = "http3")]
use parking_lot::Mutex;

#[cfg(feature = "http3")]
use crate::client::subscriptions::{Subscription, Subscriptions};

/// HTTP settings for `http://` endpoints, set with
/// `ClientBuilder::http_config`.
#[derive(Clone, Debug, Default)]
pub struct HttpConfig {
    /// Use HTTP/2 over plain TCP (prior knowledge, no upgrade) instead of
    /// HTTP/1.1. One connection is opened in `build()` and shared by all
    /// calls. A QUIC config (HTTP/3) takes priority.
    pub http2: bool,
    /// `Host` header for HTTP/1.1 requests. Not sent when `None`.
    pub host: Option<String>,
}

/// HTTP client backend for JSON-RPC.
pub(crate) struct HttpClientBackend {
    inner: HttpTransport,
    /// Shared with the Client so driver tasks (h1 per-request, h3
    /// connection) get cancelled when the Client stops.
    task_group: Arc<TaskGroup>,
}

// smol uses HTTP/1.1 via `hyper::client::conn::http1`, which is the
// smallest hyper API that compiles against `smol-hyper`. tokio gets
// HTTP/1.1 with connection pooling through `hyper-util`'s legacy
// `Client`, boxed because it is much larger than the other variants.
// HTTP/2 is opt-in and uses one shared connection on both runtimes.
enum HttpTransport {
    #[cfg(feature = "smol")]
    Smol {
        endpoint: Endpoint,
        config: HttpConfig,
    },
    #[cfg(feature = "tokio")]
    Tokio {
        endpoint: Endpoint,
        config: HttpConfig,
        client: Box<HyperClient<HttpConnector, Full<Bytes>>>,
    },
    H2 {
        endpoint: Endpoint,
        sender: h2::H2SendRequest,
    },
    #[cfg(feature = "http3")]
    H3 { send_request: h3::H3SendRequest },
}

impl HttpClientBackend {
    /// Create an HTTP/1.1 client.
    pub(crate) fn new(
        endpoint: &Endpoint,
        config: &HttpConfig,
        task_group: Arc<TaskGroup>,
    ) -> Result<Self> {
        #[cfg(feature = "smol")]
        let inner = HttpTransport::Smol {
            endpoint: endpoint.clone(),
            config: config.clone(),
        };

        #[cfg(feature = "tokio")]
        let inner = HttpTransport::Tokio {
            endpoint: endpoint.clone(),
            config: config.clone(),
            client: Box::new(
                HyperClient::builder(HyperExecutor::new(task_group.clone())).build_http(),
            ),
        };

        Ok(Self { inner, task_group })
    }

    /// Create an HTTP/2 client over plain TCP. Opens the connection now.
    pub(crate) async fn new_h2(endpoint: &Endpoint, task_group: Arc<TaskGroup>) -> Result<Self> {
        let sender = h2::connect(endpoint, &task_group).await?;
        Ok(Self {
            inner: HttpTransport::H2 {
                endpoint: endpoint.clone(),
                sender,
            },
            task_group,
        })
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
    pub(crate) async fn send_request<C: JsonRpcCodec>(
        &self,
        msg: serde_json::Value,
        codec: &C,
    ) -> Result<message::Response> {
        let _ = &self.task_group;
        match &self.inner {
            #[cfg(feature = "smol")]
            HttpTransport::Smol { endpoint, config } => {
                h1::send(endpoint, config, msg, &self.task_group, codec).await
            }
            #[cfg(feature = "tokio")]
            HttpTransport::Tokio {
                endpoint,
                config,
                client,
            } => h1::send(client, endpoint, config, msg, codec).await,
            HttpTransport::H2 { endpoint, sender } => h2::send(sender, endpoint, msg, codec).await,
            #[cfg(feature = "http3")]
            HttpTransport::H3 { send_request } => h3::send(send_request.clone(), msg, codec).await,
        }
    }

    /// Subscribe over HTTP/3. Returns the initial response + recv stream.
    #[cfg(feature = "http3")]
    pub(crate) async fn subscribe_h3<C: JsonRpcCodec>(
        &self,
        msg: serde_json::Value,
        codec: &C,
    ) -> Result<(message::Response, h3::H3SendStream, h3::H3Reader<C>)> {
        let HttpTransport::H3 { send_request } = &self.inner else {
            return Err(Error::UnsupportedProtocol(
                "subscribe_h3 requires HTTP/3".into(),
            ));
        };
        h3::subscribe(send_request.clone(), msg, codec).await
    }
}

/// Shared HTTP response parser (used by h1 and h2).
pub(super) async fn parse_response<B, C: JsonRpcCodec>(
    response: hyper::Response<B>,
    codec: &C,
) -> Result<message::Response>
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

    let rpc_response: message::Response = serde_json::from_value(decode_body(codec, &body)?)?;
    Ok(rpc_response)
}

/// Build the HTTP backend: HTTP/3 if a QUIC config is set, else
/// HTTP/2 if enabled, else HTTP/1.1.
pub(super) async fn build_backend(
    config: &ClientConfig,
    task_group: Arc<TaskGroup>,
) -> Result<ClientBackend> {
    #[cfg(feature = "http3")]
    if let Some(qc) = config.quic_config.as_ref() {
        let backend = HttpClientBackend::new_h3(&config.endpoint, qc, task_group).await?;
        return Ok(http_client_backend(config, backend));
    }

    let http_backend = if config.http_config.http2 {
        HttpClientBackend::new_h2(&config.endpoint, task_group).await?
    } else {
        HttpClientBackend::new(&config.endpoint, &config.http_config, task_group)?
    };
    Ok(http_client_backend(config, http_backend))
}

fn http_client_backend(config: &ClientConfig, http_backend: HttpClientBackend) -> ClientBackend {
    info!("HTTP client configured for endpoint: {}", config.endpoint);

    ClientBackend::Http {
        http_backend: Box::new(http_backend),
        #[cfg(feature = "http3")]
        subscriptions: Subscriptions::new(config.subscription_buffer_size),
        #[cfg(feature = "http3")]
        unsub_chans: Mutex::new(HashMap::new()),
    }
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
            Some(t) => {
                timeout(
                    Duration::from_millis(t),
                    http_backend.send_request(msg, &self.codec),
                )
                .await?
            }
            None => http_backend.send_request(msg, &self.codec).await,
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
        let ClientBackend::Http {
            subscriptions,
            unsub_chans,
            ..
        } = &self.backend
        else {
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

        let (response, send, reader) = http_backend.subscribe_h3(msg, &self.codec).await?;

        if let Some(error) = response.error {
            return Err(Error::SubscribeError(error.code, error.message));
        }

        let sub_id = match response.result {
            Some(result) => serde_json::from_value::<SubscriptionID>(result)?,
            None => return Err(Error::InvalidMsg("Invalid subscription id".into())),
        };

        let (unsub_tx, unsub_rx) = async_channel::bounded::<serde_json::Value>(1);
        unsub_chans.lock().insert(sub_id, unsub_tx);

        let sub = subscriptions.subscribe(sub_id).await;

        self.task_group.spawn_then(
            h3::subscription_task(send, reader, unsub_rx, sub.clone()),
            |res: TaskResult<Result<()>>| async move {
                log::debug!("H3 subscription task ended: {res}");
            },
        );

        Ok(sub)
    }

    /// Unsubscribe over HTTP/3. The subscription task sends the request
    /// on the subscription's own stream.
    #[cfg(feature = "http3")]
    pub(super) async fn h3_unsubscribe(&self, method: &str, sub_id: SubscriptionID) -> Result<()> {
        let ClientBackend::Http {
            subscriptions,
            unsub_chans,
            ..
        } = &self.backend
        else {
            return Err(Error::InvalidState("not in HTTP mode".into()));
        };

        // Own `let` so the MutexGuard is dropped before the `.await`.
        let ch = unsub_chans.lock().remove(&sub_id);
        if let Some(ch) = ch {
            let request = message::Request {
                jsonrpc: message::JSONRPC_VERSION.to_string(),
                id: json!(random_32()),
                method: method.to_string(),
                params: Some(json!(sub_id)),
            };
            let _ = ch.send(serde_json::to_value(request)?).await;
        }
        subscriptions.unsubscribe(&sub_id).await;
        Ok(())
    }
}
