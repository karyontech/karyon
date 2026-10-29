//! HTTP/2 client over plain TCP (h2c with prior knowledge).
//!
//! One connection is opened when the client is built and shared by
//! all requests as HTTP/2 streams. Opt in with `HttpConfig::http2`.

use std::{net::SocketAddr, sync::Arc};

use bytes::Bytes;
use http_body_util::Full;
use hyper::{
    client::conn::http2::{handshake, Connection, SendRequest},
    rt::{Read, Write},
    Request,
};
use log::error;

use karyon_core::{async_runtime::net::TcpStream, async_util::TaskGroup};
use karyon_net::Endpoint;

#[cfg(feature = "tokio")]
use hyper_util::rt::TokioIo;
#[cfg(feature = "smol")]
use smol_hyper::rt::FuturesIo;

use crate::{
    client::http::parse_response,
    codec::{encode_msg, JsonRpcCodec},
    error::{Error, Result},
    hyper_exec::HyperExecutor,
    message,
};

pub(super) type H2SendRequest = SendRequest<Full<Bytes>>;

/// Open the TCP connection and run the HTTP/2 handshake. The connection
/// driver runs in `task_group`, so it stops with the client.
pub(super) async fn connect(
    endpoint: &Endpoint,
    task_group: &Arc<TaskGroup>,
) -> Result<H2SendRequest> {
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let stream = TcpStream::connect(addr).await?;

    #[cfg(feature = "smol")]
    let io = FuturesIo::new(stream);
    #[cfg(feature = "tokio")]
    let io = TokioIo::new(stream);

    let exec = HyperExecutor::new(task_group.clone());
    let (sender, conn) = handshake(exec, io)
        .await
        .map_err(|e| Error::HttpError(e.to_string()))?;

    task_group.spawn(driver_task(conn));
    Ok(sender)
}

pub(super) async fn send<C: JsonRpcCodec>(
    sender: &H2SendRequest,
    endpoint: &Endpoint,
    msg: serde_json::Value,
    codec: &C,
) -> Result<message::Response> {
    let body = encode_msg(codec, &msg)?;

    let req = Request::post(endpoint.to_string())
        .header("Content-Type", "application/json")
        .body(Full::new(body))
        .map_err(|e| Error::HttpError(e.to_string()))?;

    // Each request is a new stream on the shared connection.
    let response = sender
        .clone()
        .send_request(req)
        .await
        .map_err(|e| Error::HttpError(e.to_string()))?;

    parse_response(response, codec).await
}

async fn driver_task<T>(conn: Connection<T, Full<Bytes>, HyperExecutor>)
where
    T: Read + Write + Unpin + Send + 'static,
{
    if let Err(err) = conn.await {
        error!("HTTP/2 client connection error: {err}");
    }
}
