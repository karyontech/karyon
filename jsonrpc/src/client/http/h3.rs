//! HTTP/3 client over QUIC via h3 / h3-quinn.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use hyper::{Request, StatusCode};
use log::debug;
use serde_json::Value;

use karyon_core::async_util::TaskGroup;
use karyon_net::{ByteBuffer, Endpoint};

use crate::{
    client::subscriptions::Subscription,
    codec::{encode_msg, extend_buf, JsonRpcCodec},
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
    task_group.spawn(driver_task(driver));

    Ok(send_request)
}

async fn driver_task(mut driver: h3::client::Connection<h3_quinn::Connection, Bytes>) {
    let closed = futures_util::future::poll_fn(|cx| driver.poll_close(cx)).await;
    debug!("H3 client driver closed: {closed}");
}

/// Reads codec frames from the recv half of an HTTP/3 request stream.
/// One read may hold part of a frame or several frames.
pub(crate) struct H3Reader<C> {
    recv: H3RecvStream,
    buf: ByteBuffer,
    codec: C,
}

impl<C: JsonRpcCodec> H3Reader<C> {
    /// Next message, or `None` when the stream ends.
    async fn next(&mut self) -> Result<Option<Value>> {
        loop {
            if let Some((n, msg)) = self.codec.decode(&mut self.buf)? {
                self.buf.advance(n);
                return Ok(Some(msg));
            }
            let chunk = self
                .recv
                .recv_data()
                .await
                .map_err(|e| Error::HttpError(format!("H3 data: {e}")))?;
            match chunk {
                Some(chunk) => extend_buf(&mut self.buf, chunk),
                None => return Ok(None),
            }
        }
    }

    /// Next message as a JSON-RPC response.
    async fn next_response(&mut self) -> Result<message::Response> {
        match self.next().await? {
            Some(msg) => Ok(serde_json::from_value(msg)?),
            None => Err(Error::HttpError("H3 stream closed before response".into())),
        }
    }
}

/// Send one request and return a reader for the reply stream.
async fn request<C: JsonRpcCodec>(
    mut send_request: H3SendRequest,
    msg: serde_json::Value,
    codec: &C,
) -> Result<H3Reader<C>> {
    let body = encode_msg(codec, &msg)?;

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
        .send_data(body)
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

    Ok(H3Reader {
        recv: stream.split().1,
        buf: ByteBuffer::new(),
        codec: codec.clone(),
    })
}

pub(super) async fn send<C: JsonRpcCodec>(
    send_request: H3SendRequest,
    msg: serde_json::Value,
    codec: &C,
) -> Result<message::Response> {
    request(send_request, msg, codec)
        .await?
        .next_response()
        .await
}

/// Subscribe over HTTP/3: send the request, read the initial response,
/// and return the reader for streaming notifications.
pub(super) async fn subscribe<C: JsonRpcCodec>(
    send_request: H3SendRequest,
    msg: serde_json::Value,
    codec: &C,
) -> Result<(message::Response, H3Reader<C>)> {
    let mut reader = request(send_request, msg, codec).await?;
    let rpc_response = reader.next_response().await?;
    Ok((rpc_response, reader))
}

pub(super) async fn notification_reader_task<C: JsonRpcCodec>(
    reader: H3Reader<C>,
    sub: Arc<Subscription>,
) -> Result<()> {
    read_notifications(reader, &sub).await
}

async fn read_notifications<C: JsonRpcCodec>(
    mut reader: H3Reader<C>,
    sub: &Subscription,
) -> Result<()> {
    while let Some(msg) = reader.next().await? {
        let nt: message::Notification = match serde_json::from_value(msg) {
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
