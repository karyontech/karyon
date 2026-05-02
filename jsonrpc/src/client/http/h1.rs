//! HTTP/1.1 client over smol. No connection pooling: each request
//! opens a fresh TCP connection.

use std::net::SocketAddr;

use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;

use karyon_core::{
    async_runtime::net::TcpStream,
    async_util::{TaskGroup, TaskResult},
};
use karyon_net::Endpoint;
use smol_hyper::rt::FuturesIo;

use crate::{
    client::http::parse_response,
    error::{Error, Result},
    message,
};

pub(super) async fn send(
    endpoint: &Endpoint,
    msg: serde_json::Value,
    task_group: &TaskGroup,
) -> Result<message::Response> {
    let body = serde_json::to_vec(&msg)?;

    // Resolve the endpoint to a SocketAddr (handles both literal IPs
    // and domain names via the system DNS).
    let addr = SocketAddr::try_from(endpoint.clone())?;

    let stream = TcpStream::connect(addr).await?;
    let io = FuturesIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| Error::HttpError(e.to_string()))?;

    // hyper's HTTP/1.1 client splits I/O from request building: the
    // returned `conn` future drives the wire protocol and must be
    // polled concurrently, otherwise `sender` blocks forever. Spawn
    // it via the Client task_group so it gets cancelled on stop.
    task_group.spawn(driver_task(conn), |_: TaskResult<()>| async {});

    let req = Request::post(endpoint.to_string())
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .map_err(|e| Error::HttpError(e.to_string()))?;

    let response = sender
        .send_request(req)
        .await
        .map_err(|e| Error::HttpError(e.to_string()))?;

    parse_response(response).await
}

async fn driver_task<T>(conn: hyper::client::conn::http1::Connection<T, Full<Bytes>>)
where
    T: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    if let Err(err) = conn.await {
        log::error!("HTTP client connection error: {err}");
    }
}
