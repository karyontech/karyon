//! HTTP/3 client over QUIC via h3 / h3-quinn.

use std::net::SocketAddr;
use std::sync::Arc;

use async_channel::Receiver;
use bytes::Bytes;
use h3::client::{RequestStream, SendRequest};
use h3_quinn::{BidiStream, SendStream};
use hyper::{header::CONTENT_TYPE, Request, StatusCode};
use log::debug;
use serde_json::Value;

use karyon_core::async_util::{select, Either, TaskGroup};
use karyon_net::{ByteBuffer, Endpoint};

use crate::{
    client::http::HttpConfig,
    client::subscriptions::{decode_notification, NotificationDecoder, Subscription},
    codec::{encode_msg, extend_buf, JsonRpcCodec},
    error::{Error, Result},
    message,
};

pub(super) type H3SendRequest = SendRequest<h3_quinn::OpenStreams, Bytes>;
pub(super) type H3RecvStream = RequestStream<h3_quinn::RecvStream, Bytes>;
pub(super) type H3SendStream = RequestStream<SendStream<Bytes>, Bytes>;
type H3Stream = RequestStream<BidiStream<Bytes>, Bytes>;

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

/// Send the request headers and `msg`. The request body stays open.
async fn start_request<C: JsonRpcCodec>(
    mut send_request: H3SendRequest,
    config: &HttpConfig,
    msg: serde_json::Value,
    codec: &C,
) -> Result<H3Stream> {
    let body = encode_msg(codec, &msg)?;

    // The QUIC connection is already established, so the URI's scheme
    // and authority are only filled in to satisfy `Request::post`'s
    // parser; the server only routes on `:path`.
    let req = Request::post("https://localhost/")
        .header(CONTENT_TYPE, config.content_type.as_str())
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

    Ok(stream)
}

/// Wait for the response headers and split the stream into the send
/// half and a reader for the reply.
async fn read_response<C: JsonRpcCodec>(
    mut stream: H3Stream,
    codec: &C,
) -> Result<(H3SendStream, H3Reader<C>)> {
    let resp = stream
        .recv_response()
        .await
        .map_err(|e| Error::HttpError(format!("H3 recv: {e}")))?;

    if resp.status() != StatusCode::OK {
        return Err(Error::HttpError(format!("HTTP/3 error: {}", resp.status())));
    }

    let (send, recv) = stream.split();
    let reader = H3Reader {
        recv,
        buf: ByteBuffer::new(),
        codec: codec.clone(),
    };
    Ok((send, reader))
}

pub(super) async fn send<C: JsonRpcCodec>(
    send_request: H3SendRequest,
    config: &HttpConfig,
    msg: serde_json::Value,
    codec: &C,
) -> Result<message::Response> {
    let mut stream = start_request(send_request, config, msg, codec).await?;
    // Finish before reading, so servers that read the whole body first
    // still answer.
    stream
        .finish()
        .await
        .map_err(|e| Error::HttpError(format!("H3 finish: {e}")))?;
    let (_, mut reader) = read_response(stream, codec).await?;
    reader.next_response().await
}

/// Subscribe over HTTP/3: send the request with the body left open, read
/// the initial response, and return the send half and the reader for
/// notifications.
pub(super) async fn subscribe<C: JsonRpcCodec>(
    send_request: H3SendRequest,
    config: &HttpConfig,
    msg: serde_json::Value,
    codec: &C,
) -> Result<(message::Response, H3SendStream, H3Reader<C>)> {
    let stream = start_request(send_request, config, msg, codec).await?;
    let (send, mut reader) = read_response(stream, codec).await?;
    let rpc_response = reader.next_response().await?;
    Ok((rpc_response, send, reader))
}

/// Forward notifications to the subscription until the stream ends or
/// an unsubscribe request arrives. The unsubscribe goes out on the same
/// stream, so the server handles it with the subscription's channel.
// TODO: remove the subscription when this task exits; now `recv()`
// waits forever once the stream ends.
pub(super) async fn subscription_task<C: JsonRpcCodec>(
    mut send: H3SendStream,
    mut reader: H3Reader<C>,
    unsub_rx: Receiver<Value>,
    sub: Arc<Subscription>,
    decoder: NotificationDecoder,
) -> Result<()> {
    loop {
        match select(reader.next(), unsub_rx.recv()).await {
            Either::Left(Ok(Some(msg))) => {
                let nt_res = match decode_notification(decoder, msg) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let val = nt_res.result.unwrap_or(serde_json::json!(""));
                if sub.notify(val).await.is_err() {
                    break;
                }
            }
            Either::Left(_) => break,
            Either::Right(unsub_msg) => {
                if let Ok(msg) = unsub_msg {
                    let body = encode_msg(&reader.codec, &msg)?;
                    let _ = send.send_data(body).await;
                    let _ = send.finish().await;
                }
                break;
            }
        }
    }
    Ok(())
}
