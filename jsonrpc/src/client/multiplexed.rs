//! Multiplexed client mode (TCP/TLS/Unix and WS/WSS).
//! A single message connection is shared across all calls and
//! subscriptions. A background task splits reader from writer.

use std::sync::{atomic::Ordering, Arc};

use async_channel::Receiver;
use log::{debug, error, info};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;

use karyon_core::{
    async_util::{select, timeout, Either, TaskResult},
    util::random_32,
};

use karyon_net::{framed, Endpoint, FramedConn, MessageRx, MessageTx};

#[cfg(feature = "ws")]
use karyon_net::{layers::ws::WsConn, ClientLayer};

use crate::{
    client::{
        message_dispatcher::MessageDispatcher, subscriptions::Subscriptions, Client, ClientBackend,
        ClientConfig, RequestID, WsCodec,
    },
    codec::JsonRpcCodec,
    error::{Error, Result},
    message,
};

#[cfg(feature = "ws")]
use crate::codec::JsonRpcWsCodec;

/// Capacity of the outbound queue feeding the writer task. Small
/// because each in-flight call already has its own response channel,
/// so this only buffers bursts between caller and writer.
const OUTBOUND_BUFFER_SIZE: usize = 10;

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum NewMsg {
    Notification(message::Notification),
    Response(message::Response),
}

/// Build the multiplexed backend by connecting over a byte stream
/// (TCP, TLS, or Unix) and framing it.
pub(super) async fn build_byte_backend<B>(
    config: &ClientConfig,
    codec: B,
) -> Result<(ClientBackend, FramedConn<B>)>
where
    B: JsonRpcCodec,
{
    let conn = connect_byte(config, codec).await?;
    let peer = conn.peer_endpoint();
    info!("Successfully connected to the RPC server: {peer:?}");

    let backend = build_backend_state(config);
    Ok((backend, conn))
}

/// Build the multiplexed backend by connecting over a WebSocket.
#[cfg(feature = "ws")]
pub(super) async fn build_ws_backend<W>(
    config: &ClientConfig,
    codec: W,
) -> Result<(ClientBackend, WsConn<W>)>
where
    W: JsonRpcWsCodec,
{
    let conn = connect_ws(config, codec).await?;
    let peer = conn.peer_endpoint();
    info!("Successfully connected to the RPC server: {peer:?}");

    let backend = build_backend_state(config);
    Ok((backend, conn))
}

fn build_backend_state(config: &ClientConfig) -> ClientBackend {
    ClientBackend::Multiplexed {
        message_dispatcher: MessageDispatcher::new(),
        subscriptions: Subscriptions::new(config.subscription_buffer_size),
        send_chan: async_channel::bounded(OUTBOUND_BUFFER_SIZE),
    }
}

/// Spawn the reader/writer task that drives the multiplexed wire.
pub(super) fn start_io_loop<B, W, R, Wr>(client: &Arc<Client<B, W>>, reader: R, writer: Wr)
where
    B: JsonRpcCodec,
    W: WsCodec,
    R: MessageRx<Message = serde_json::Value> + Send + 'static,
    Wr: MessageTx<Message = serde_json::Value> + Send + 'static,
{
    let this = client.clone();
    client.task_group.spawn(
        background_loop(client.clone(), reader, writer),
        move |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("Client stopped: {err}");
            }
            this.disconnect.store(true, Ordering::Relaxed);
            if let ClientBackend::Multiplexed {
                subscriptions,
                message_dispatcher,
                ..
            } = &this.backend
            {
                subscriptions.clear().await;
                message_dispatcher.clear().await;
            }
        },
    );
}

async fn background_loop<B, W, R, Wr>(
    client: Arc<Client<B, W>>,
    reader: R,
    writer: Wr,
) -> Result<()>
where
    B: JsonRpcCodec,
    W: WsCodec,
    R: MessageRx<Message = serde_json::Value> + Send,
    Wr: MessageTx<Message = serde_json::Value> + Send,
{
    let ClientBackend::Multiplexed {
        message_dispatcher,
        subscriptions,
        send_chan,
    } = &client.backend
    else {
        return Err(Error::InvalidState("not in multiplexed mode".into()));
    };

    run_io_loop(
        reader,
        writer,
        send_chan.1.clone(),
        message_dispatcher,
        subscriptions,
    )
    .await
}

async fn run_io_loop<R, Wr>(
    mut reader: R,
    mut writer: Wr,
    outbound: Receiver<serde_json::Value>,
    message_dispatcher: &MessageDispatcher,
    subscriptions: &Subscriptions,
) -> Result<()>
where
    R: MessageRx<Message = serde_json::Value> + Send,
    Wr: MessageTx<Message = serde_json::Value> + Send,
{
    loop {
        match select(outbound.recv(), reader.recv_msg()).await {
            Either::Left(req) => {
                writer.send_msg(req?).await?;
            }
            Either::Right(msg) => {
                match handle_mux_msg(message_dispatcher, subscriptions, msg?).await {
                    Err(Error::SubscriptionBufferFull) => {
                        return Err(Error::SubscriptionBufferFull);
                    }
                    Err(err) => {
                        let ep = reader.peer_endpoint();
                        error!("Handle msg from {ep:?}: {err}");
                    }
                    Ok(_) => {}
                }
            }
        }
    }
}

async fn handle_mux_msg(
    message_dispatcher: &MessageDispatcher,
    subscriptions: &Subscriptions,
    msg: serde_json::Value,
) -> Result<()> {
    match serde_json::from_value::<NewMsg>(msg.clone()) {
        Ok(NewMsg::Response(res)) => {
            debug!("<-- {res}");
            message_dispatcher.dispatch(res).await
        }
        Ok(NewMsg::Notification(nt)) => {
            debug!("<-- {nt}");
            subscriptions.notify(nt).await
        }
        Err(err) => {
            error!("Receive unexpected msg {msg}: {err}");
            Err(Error::InvalidMsg("Unexpected msg".to_string()))
        }
    }
}

async fn connect_byte<B>(config: &ClientConfig, codec: B) -> Result<FramedConn<B>>
where
    B: JsonRpcCodec,
{
    let endpoint = config.endpoint.clone();

    match &endpoint {
        #[cfg(feature = "tcp")]
        Endpoint::Tcp(..) => {
            let stream = karyon_net::tcp::connect(&endpoint, config.tcp_config.clone()).await?;
            Ok(framed(stream, codec))
        }
        #[cfg(feature = "tls")]
        Endpoint::Tls(..) => {
            let stream = karyon_net::tcp::connect(&endpoint, config.tcp_config.clone()).await?;
            let tls_config = config.tls_config.as_ref().ok_or(Error::TLSConfigRequired)?;
            let tls_layer = karyon_net::tls::TlsLayer::client(tls_config.clone());
            let tls_stream = karyon_net::ClientLayer::handshake(&tls_layer, stream).await?;
            Ok(framed(tls_stream, codec))
        }
        #[cfg(all(feature = "unix", target_family = "unix"))]
        Endpoint::Unix(..) => {
            let stream = karyon_net::unix::connect(&endpoint).await?;
            Ok(framed(stream, codec))
        }
        _ => Err(Error::UnsupportedProtocol(endpoint.to_string())),
    }
}

#[cfg(feature = "ws")]
async fn connect_ws<W>(config: &ClientConfig, codec: W) -> Result<WsConn<W>>
where
    W: JsonRpcWsCodec,
{
    let endpoint = config.endpoint.clone();
    let url = endpoint.to_string();

    match &endpoint {
        Endpoint::Ws(..) => {
            let stream = karyon_net::tcp::connect(&endpoint, config.tcp_config.clone()).await?;
            let layer = karyon_net::layers::ws::WsLayer::client(&url, codec);
            Ok(ClientLayer::handshake(&layer, stream).await?)
        }
        #[cfg(feature = "tls")]
        Endpoint::Wss(..) => {
            let stream = karyon_net::tcp::connect(&endpoint, config.tcp_config.clone()).await?;
            let tls_config = config.tls_config.as_ref().ok_or(Error::TLSConfigRequired)?;
            let tls_layer = karyon_net::tls::TlsLayer::client(tls_config.clone());
            let tls_stream = karyon_net::ClientLayer::handshake(&tls_layer, stream).await?;
            let layer = karyon_net::layers::ws::WsLayer::client(&url, codec);
            Ok(ClientLayer::handshake(&layer, tls_stream).await?)
        }
        _ => Err(Error::UnsupportedProtocol(endpoint.to_string())),
    }
}

pub(super) async fn send_request<B, W, T>(
    client: &Client<B, W>,
    method: &str,
    params: T,
) -> Result<message::Response>
where
    B: JsonRpcCodec,
    W: WsCodec,
    T: Serialize + DeserializeOwned,
{
    let ClientBackend::Multiplexed {
        message_dispatcher,
        send_chan,
        ..
    } = &client.backend
    else {
        return Err(Error::InvalidState("not in multiplexed mode".into()));
    };

    let id: RequestID = random_32();
    let request = message::Request {
        jsonrpc: message::JSONRPC_VERSION.to_string(),
        id: json!(id),
        method: method.to_string(),
        params: Some(json!(params)),
    };

    if client.disconnect.load(Ordering::Relaxed) {
        return Err(Error::ClientDisconnected);
    }
    let req = serde_json::to_value(request)?;
    send_chan.0.send(req).await?;

    let rx = message_dispatcher.register(id).await;

    let result = match client.config.timeout {
        Some(t) => timeout(std::time::Duration::from_millis(t), rx.recv()).await?,
        None => rx.recv().await,
    };

    let response = match result {
        Ok(r) => r,
        Err(err) => {
            message_dispatcher.unregister(&id).await;
            return Err(err.into());
        }
    };

    if let Some(error) = response.error {
        return Err(Error::SubscribeError(error.code, error.message));
    }

    let resp_id = response
        .id
        .as_ref()
        .ok_or_else(|| Error::InvalidMsg("Missing response id".to_string()))?;
    if *resp_id != id {
        return Err(Error::InvalidMsg("Invalid response id".to_string()));
    }

    Ok(response)
}
