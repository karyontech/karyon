//! HTTP/3 over QUIC via h3 / h3-quinn.
//!
//! One request per stream. A subscription keeps its stream open:
//! notifications go out on the response, and the client sends the
//! unsubscribe request on the same request body.

use std::{fmt::Display, sync::Arc};

use async_channel::bounded;
use bytes::Bytes;
use h3::server::{Connection as H3Connection, RequestResolver, RequestStream};
use h3_quinn::{Connection, RecvStream, SendStream};
use hyper::{header::CONTENT_TYPE, Method, Request, Response, StatusCode};
use log::{debug, error};
use serde_json::{json, to_value, Value};

use karyon_core::async_util::{select, Either};

use karyon_net::{
    quic::{QuicConn, QuicEndpoint, QuicIncoming},
    ByteBuffer, Endpoint,
};

use crate::{
    codec::{encode_msg, extend_buf, JsonRpcCodec, DEFAULT_CONTENT_TYPE},
    error::{Error, Result},
    message,
    server::{
        channel::Channel,
        dispatch::{sanity_check, Handler, NewRequest, SanityCheckResult},
        http::{
            encode_response, parse_error_response, ERR_BODY_TOO_LARGE, ERR_METHOD_NOT_ALLOWED,
            MAX_HTTP_BODY_SIZE,
        },
        Server, CHANNEL_SUBSCRIPTION_BUFFER_SIZE,
    },
};

type H3Resolver = RequestResolver<Connection, Bytes>;
type H3Send = RequestStream<SendStream<Bytes>, Bytes>;
type H3Recv = RequestStream<RecvStream, Bytes>;

pub(super) async fn accept_h3<C: JsonRpcCodec>(
    server: &Arc<Server>,
    quic_ep: &QuicEndpoint,
    codec: &C,
) {
    // Only the accept runs here; the handshake runs in the spawned task.
    let incoming = match quic_ep.accept_incoming().await {
        Ok(incoming) => incoming,
        Err(err) => {
            error!("Accept QUIC for HTTP/3: {err}");
            return;
        }
    };

    server
        .task_group
        .spawn(serve_incoming_task(server.clone(), incoming, codec.clone()));
}

async fn serve_incoming_task<C: JsonRpcCodec>(
    server: Arc<Server>,
    incoming: QuicIncoming,
    codec: C,
) -> Result<()> {
    let peer = incoming.peer_endpoint();
    match incoming.handshake().await {
        Ok(quic_conn) => serve_conn_task(server, quic_conn, codec).await,
        Err(err) => {
            debug!("QUIC handshake with {peer} failed: {err}");
            Ok(())
        }
    }
}

async fn serve_conn_task<C: JsonRpcCodec>(
    server: Arc<Server>,
    quic_conn: QuicConn,
    codec: C,
) -> Result<()> {
    let peer = quic_conn
        .peer_endpoint()
        .map(|e| e.to_string())
        .unwrap_or_default();
    if let Err(err) = serve_conn(server, quic_conn, codec).await {
        debug!("HTTP/3 from {peer} closed: {err}");
    }
    Ok(())
}

async fn serve_conn<C: JsonRpcCodec>(
    server: Arc<Server>,
    quic_conn: QuicConn,
    codec: C,
) -> Result<()> {
    let h3_conn = Connection::new(quic_conn.inner().clone());
    let mut h3_server: H3Connection<Connection, Bytes> = H3Connection::new(h3_conn)
        .await
        .map_err(|e| Error::HttpError(format!("H3 handshake: {e}")))?;

    let peer = quic_conn.peer_endpoint().ok();

    loop {
        match h3_server.accept().await {
            // Headers are read in the request task, so a slow stream
            // does not block the other streams of this connection.
            Ok(Some(resolver)) => {
                server.task_group.spawn(handle_request_task(
                    server.clone(),
                    resolver,
                    peer.clone(),
                    codec.clone(),
                ));
            }
            Ok(None) => break,
            Err(err) => {
                debug!("HTTP/3 connection error: {err}");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_request_task<C: JsonRpcCodec>(
    server: Arc<Server>,
    resolver: H3Resolver,
    peer: Option<Endpoint>,
    codec: C,
) -> Result<()> {
    let (req, stream) = match resolver.resolve_request().await {
        Ok(v) => v,
        Err(err) => {
            debug!("Resolve HTTP/3 request: {err}");
            return Ok(());
        }
    };

    let (send, recv) = stream.split();
    if let Err(err) = handle_h3_request(server, req, send, recv, peer, codec).await {
        error!("Handle HTTP/3 request: {err}");
    }
    Ok(())
}

async fn handle_h3_request<C: JsonRpcCodec>(
    server: Arc<Server>,
    req: Request<()>,
    mut send: H3Send,
    recv: H3Recv,
    peer: Option<Endpoint>,
    codec: C,
) -> Result<()> {
    let content_type = server.config.http_config.content_type.clone();

    if req.method() != Method::POST {
        let body = ERR_METHOD_NOT_ALLOWED.as_bytes();
        return h3_send(
            &mut send,
            StatusCode::METHOD_NOT_ALLOWED,
            DEFAULT_CONTENT_TYPE,
            body,
        )
        .await;
    }

    // Read only the first message; a subscription keeps the body open.
    let mut reader = BodyReader {
        recv,
        buf: ByteBuffer::new(),
        codec: codec.clone(),
    };
    let msg = match reader.next().await {
        Ok(Some(msg)) => msg,
        Err(Error::BufferFull(_)) => {
            let body = ERR_BODY_TOO_LARGE.as_bytes();
            return h3_send(
                &mut send,
                StatusCode::PAYLOAD_TOO_LARGE,
                DEFAULT_CONTENT_TYPE,
                body,
            )
            .await;
        }
        Err(Error::HttpError(err)) => return Err(Error::HttpError(err)),
        Ok(None) | Err(_) => {
            let body = encode_response(&codec, &parse_error_response())?;
            return h3_send(&mut send, StatusCode::OK, &content_type, &body).await;
        }
    };

    let req = match sanity_check(msg) {
        SanityCheckResult::NewReq(req) => req,
        SanityCheckResult::ErrRes(res) => {
            let body = encode_response(&codec, &res)?;
            return h3_send(&mut send, StatusCode::OK, &content_type, &body).await;
        }
    };

    // Pubsub is detected by the resolved handler, not the method name.
    // TODO: match on the handler and pass it on. Now it is resolved here,
    // then the request is re-encoded, checked and resolved again in
    // `handle_request`.
    let is_pubsub = matches!(
        server.resolve_handler(&req.srvc_name, &req.method_name, true),
        Handler::Pubsub(_)
    );
    if is_pubsub {
        return handle_h3_subscription(server, req, send, reader, peer, codec).await;
    }

    let msg = to_value(&req.msg)?;
    let response = server.handle_request(None, msg).await;
    debug!("--> {response}");
    let body = encode_response(&codec, &response)?;
    h3_send(&mut send, StatusCode::OK, &content_type, &body).await
}

/// Pubsub over HTTP/3: the subscription gets its own `Channel`.
/// Notifications stream on the response while the request body waits
/// for the unsubscribe request.
async fn handle_h3_subscription<C: JsonRpcCodec>(
    server: Arc<Server>,
    req: NewRequest,
    mut send: H3Send,
    mut reader: BodyReader<C>,
    peer: Option<Endpoint>,
    codec: C,
) -> Result<()> {
    let content_type = server.config.http_config.content_type.clone();
    let (ch_tx, ch_rx) = bounded(CHANNEL_SUBSCRIPTION_BUFFER_SIZE);
    let channel = Channel::new(ch_tx, peer);

    let method = match server.resolve_handler(&req.srvc_name, &req.method_name, true) {
        Handler::Pubsub(m) => m,
        _ => unreachable!("pubsub method presence checked by caller"),
    };

    let params = req.msg.params.unwrap_or(json!(()));
    let response = match method(channel.clone(), req.msg.method, params).await {
        Ok(res) => message::Response {
            result: Some(res),
            id: Some(req.msg.id),
            ..Default::default()
        },
        Err(err) => {
            let response = err.to_response(Some(req.msg.id), None);
            let body = encode_response(&codec, &response)?;
            return h3_send(&mut send, StatusCode::OK, &content_type, &body).await;
        }
    };

    debug!("--> {response}");
    send.send_response(ok_response(&content_type)?)
        .await
        .map_err(h3_err)?;
    let body = encode_response(&codec, &response)?;
    send.send_data(body).await.map_err(h3_err)?;

    let encoder = server.config.notification_encoder;
    let result = loop {
        match select(ch_rx.recv(), reader.next()).await {
            Either::Left(Ok(nt)) => {
                let notification = encoder(nt);
                debug!("--> {notification}");
                let body = encode_msg(&codec, &json!(notification))?;
                if let Err(err) = send.send_data(body).await {
                    break Err(h3_err(err));
                }
            }
            Either::Left(Err(err)) => break Err(err.into()),
            // The unsubscribe request, handled with the subscription's
            // own channel.
            Either::Right(Ok(Some(msg))) => {
                let response = server.handle_request(Some(channel.clone()), msg).await;
                debug!("--> {response}");
                let body = encode_response(&codec, &response)?;
                break send.send_data(body).await.map_err(h3_err);
            }
            Either::Right(_) => break Ok(()),
        }
    };

    channel.close();
    let _ = send.finish().await;
    result
}

/// Reads codec frames from an HTTP/3 request body.
struct BodyReader<C> {
    recv: H3Recv,
    buf: ByteBuffer,
    codec: C,
}

impl<C: JsonRpcCodec> BodyReader<C> {
    /// Next message, or `None` when the body ends.
    async fn next(&mut self) -> Result<Option<Value>> {
        loop {
            if let Some((n, msg)) = self.codec.decode(&mut self.buf)? {
                self.buf.advance(n);
                return Ok(Some(msg));
            }
            if self.buf.len() as u64 > MAX_HTTP_BODY_SIZE {
                return Err(Error::BufferFull("request body too large".into()));
            }
            match self.recv.recv_data().await.map_err(h3_err)? {
                Some(chunk) => extend_buf(&mut self.buf, chunk),
                None => return Ok(None),
            }
        }
    }
}

fn ok_response(content_type: &str) -> Result<Response<()>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .body(())
        .map_err(|e| Error::HttpError(e.to_string()))
}

async fn h3_send(
    send: &mut H3Send,
    status: StatusCode,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    let resp = Response::builder()
        .status(status)
        .header(CONTENT_TYPE, content_type)
        .body(())
        .map_err(|e| Error::HttpError(e.to_string()))?;
    send.send_response(resp).await.map_err(h3_err)?;
    send.send_data(Bytes::from(body.to_vec()))
        .await
        .map_err(h3_err)?;
    send.finish().await.map_err(h3_err)?;
    Ok(())
}

fn h3_err(e: impl Display) -> Error {
    Error::HttpError(e.to_string())
}
