//! HTTP/3 client over QUIC via h3 / h3-quinn.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::{Buf, Bytes};
use hyper::{Request, StatusCode};
use log::debug;

use karyon_core::async_util::{TaskGroup, TaskResult};
use karyon_net::Endpoint;

use crate::{
    client::subscriptions::Subscription,
    error::{Error, Result},
    message,
};

pub(super) type H3SendRequest = h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>;
pub(super) type H3RecvStream = h3::client::RequestStream<h3_quinn::RecvStream, Bytes>;

/// Connect to an HTTP/3 server and return a request sender. A
/// background task drives the connection until close.
pub(super) async fn connect(
    endpoint: &Endpoint,
    quic_config: &karyon_net::quic::ClientQuicConfig,
    task_group: Arc<TaskGroup>,
) -> Result<H3SendRequest> {
    let quic_ep = Endpoint::new_quic_addr(SocketAddr::try_from(endpoint.clone())?);
    let quic_conn = karyon_net::quic::QuicEndpoint::dial(&quic_ep, quic_config.clone()).await?;

    let h3_conn = h3_quinn::Connection::new(quic_conn.inner().clone());
    let (driver, send_request) = h3::client::new(h3_conn)
        .await
        .map_err(|e| Error::HttpError(format!("H3 handshake: {e}")))?;

    // h3 splits I/O from the request API: `driver` polls the QUIC
    // connection in the background; without it `send_request` would
    // never make progress. Spawn via the Client task_group so it
    // gets cancelled on stop.
    task_group.spawn(driver_task(driver), |_: TaskResult<()>| async {});

    Ok(send_request)
}

async fn driver_task(mut driver: h3::client::Connection<h3_quinn::Connection, Bytes>) {
    let closed = futures_util::future::poll_fn(|cx| driver.poll_close(cx)).await;
    debug!("H3 client driver closed: {closed}");
}

pub(super) async fn send(
    mut send_request: H3SendRequest,
    msg: serde_json::Value,
) -> Result<message::Response> {
    let body = serde_json::to_vec(&msg)?;

    // The QUIC connection is already established, so the URI's scheme
    // and authority are only filled in to satisfy `Request::post`'s
    // parser; the server only routes on `:path`.
    let req = Request::post("https://localhost/")
        .header("Content-Type", "application/json")
        .body(())
        .map_err(|e| Error::HttpError(e.to_string()))?;

    let mut stream = send_request
        .send_request(req)
        .await
        .map_err(|e| Error::HttpError(format!("H3 send: {e}")))?;

    stream
        .send_data(Bytes::from(body))
        .await
        .map_err(|e| Error::HttpError(format!("H3 data: {e}")))?;

    stream
        .finish()
        .await
        .map_err(|e| Error::HttpError(format!("H3 finish: {e}")))?;

    let resp = stream
        .recv_response()
        .await
        .map_err(|e| Error::HttpError(format!("H3 recv: {e}")))?;

    if resp.status() != StatusCode::OK {
        return Err(Error::HttpError(format!("HTTP/3 error: {}", resp.status())));
    }

    let mut resp_body = Vec::new();
    while let Some(chunk) = stream
        .recv_data()
        .await
        .map_err(|e| Error::HttpError(format!("H3 data: {e}")))?
    {
        resp_body.extend_from_slice(Buf::chunk(&chunk));
    }

    let rpc_response: message::Response = serde_json::from_slice(&resp_body)?;
    Ok(rpc_response)
}

/// Subscribe over HTTP/3: send the request, read the initial response,
/// and return the recv half for streaming notifications.
pub(super) async fn subscribe(
    mut send_request: H3SendRequest,
    msg: serde_json::Value,
) -> Result<(message::Response, H3RecvStream)> {
    let body = serde_json::to_vec(&msg)?;

    let req = Request::post("https://localhost/")
        .header("Content-Type", "application/json")
        .body(())
        .map_err(|e| Error::HttpError(e.to_string()))?;

    let mut stream = send_request
        .send_request(req)
        .await
        .map_err(|e| Error::HttpError(format!("H3 send: {e}")))?;

    stream
        .send_data(Bytes::from(body))
        .await
        .map_err(|e| Error::HttpError(format!("H3 data: {e}")))?;

    stream
        .finish()
        .await
        .map_err(|e| Error::HttpError(format!("H3 finish: {e}")))?;

    let resp = stream
        .recv_response()
        .await
        .map_err(|e| Error::HttpError(format!("H3 recv: {e}")))?;

    if resp.status() != StatusCode::OK {
        return Err(Error::HttpError(format!("HTTP/3 error: {}", resp.status())));
    }

    let first = stream
        .recv_data()
        .await
        .map_err(|e| Error::HttpError(format!("H3 data: {e}")))?
        .ok_or_else(|| Error::HttpError("H3 stream closed before response".into()))?;

    let rpc_response: message::Response = serde_json::from_slice(Buf::chunk(&first))?;
    let recv_stream = stream.split().1;
    Ok((rpc_response, recv_stream))
}

pub(super) async fn notification_reader_task(
    recv: H3RecvStream,
    sub: Arc<Subscription>,
) -> Result<()> {
    read_notifications(recv, &sub).await
}

async fn read_notifications(mut recv: H3RecvStream, sub: &Subscription) -> Result<()> {
    while let Some(chunk) = recv
        .recv_data()
        .await
        .map_err(|e| Error::HttpError(format!("H3 notification: {e}")))?
    {
        let nt: message::Notification = match serde_json::from_slice(Buf::chunk(&chunk)) {
            Ok(v) => v,
            Err(_) => continue,
        };
        debug!("<-- {nt}");
        let nt_res: message::NotificationResult = match nt.params {
            Some(ref p) => match serde_json::from_value(p.clone()) {
                Ok(r) => r,
                Err(_) => continue,
            },
            None => continue,
        };
        let val = nt_res.result.unwrap_or(serde_json::json!(""));
        if sub.notify(val).await.is_err() {
            break;
        }
    }
    Ok(())
}
