//! HTTP/1.1 client.
//!
//! - smol: hyper's `client::conn::http1`. No connection pooling: each
//!   request opens a fresh TCP connection.
//! - tokio: `hyper_util::client::legacy::Client`, which handles DNS and
//!   reuses connections. hyper's high-level Client needs a tokio
//!   runtime, so smol cannot use it.

#[cfg(feature = "smol")]
use std::net::SocketAddr;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{
    header::{CONTENT_TYPE, HOST},
    Request,
};

#[cfg(feature = "smol")]
use hyper::{
    client::conn::http1::{handshake, Connection},
    rt::{Read, Write},
};
#[cfg(feature = "tokio")]
use hyper_util::client::legacy::{connect::HttpConnector, Client as HyperClient};
#[cfg(feature = "smol")]
use log::error;
#[cfg(feature = "smol")]
use smol_hyper::rt::FuturesIo;

#[cfg(feature = "smol")]
use karyon_core::{async_runtime::net::TcpStream, async_util::TaskGroup};
use karyon_net::Endpoint;

use crate::{
    client::http::{parse_response, HttpConfig},
    codec::{encode_msg, JsonRpcCodec},
    error::{Error, Result},
    message,
};

#[cfg(feature = "smol")]
pub(super) async fn send<C: JsonRpcCodec>(
    endpoint: &Endpoint,
    config: &HttpConfig,
    msg: serde_json::Value,
    task_group: &TaskGroup,
    codec: &C,
) -> Result<message::Response> {
    let req = build_request(endpoint, config, &msg, codec)?;

    // Resolve the endpoint to a SocketAddr (handles both literal IPs
    // and domain names via the system DNS).
    let addr = SocketAddr::try_from(endpoint.clone())?;

    let stream = TcpStream::connect(addr).await?;
    let io = FuturesIo::new(stream);

    let (mut sender, conn) = handshake(io)
        .await
        .map_err(|e| Error::HttpError(e.to_string()))?;

    // hyper's HTTP/1.1 client splits I/O from request building: the
    // returned `conn` future drives the wire protocol and must be
    // polled concurrently, otherwise `sender` blocks forever. Spawn
    // it via the Client task_group so it gets cancelled on stop.
    task_group.spawn(driver_task(conn));

    let response = sender
        .send_request(req)
        .await
        .map_err(|e| Error::HttpError(e.to_string()))?;

    parse_response(response, codec).await
}

#[cfg(feature = "tokio")]
pub(super) async fn send<C: JsonRpcCodec>(
    client: &HyperClient<HttpConnector, Full<Bytes>>,
    endpoint: &Endpoint,
    config: &HttpConfig,
    msg: serde_json::Value,
    codec: &C,
) -> Result<message::Response> {
    let req = build_request(endpoint, config, &msg, codec)?;

    let response = client
        .request(req)
        .await
        .map_err(|e| Error::HttpError(e.to_string()))?;

    parse_response(response, codec).await
}

/// POST request with the encoded message as body.
fn build_request<C: JsonRpcCodec>(
    endpoint: &Endpoint,
    config: &HttpConfig,
    msg: &serde_json::Value,
    codec: &C,
) -> Result<Request<Full<Bytes>>> {
    let body = encode_msg(codec, msg)?;

    let mut req =
        Request::post(endpoint.to_string()).header(CONTENT_TYPE, config.content_type.as_str());
    if let Some(host) = &config.host {
        req = req.header(HOST, host);
    }
    req.body(Full::new(body))
        .map_err(|e| Error::HttpError(e.to_string()))
}

#[cfg(feature = "smol")]
async fn driver_task<T>(conn: Connection<T, Full<Bytes>>)
where
    T: Read + Write + Unpin + Send + 'static,
{
    if let Err(err) = conn.await {
        error!("HTTP client connection error: {err}");
    }
}
