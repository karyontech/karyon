//! QUIC client mode: one stream per call, one stream per subscription.

use std::sync::{Arc, atomic::Ordering};

use async_channel::Receiver;
use log::{debug, info};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use karyon_core::{
    async_util::{Either, TaskResult, select},
    util::random_32,
};

use karyon_net::{
    FramedReader, FramedWriter, StreamMux, framed,
    quic::{QuicConn, QuicEndpoint},
};

use crate::{
    client::{
        Client, ClientBackend, ClientConfig, RequestID, WsCodec,
        subscriptions::{NotificationDecoder, Subscription, Subscriptions, decode_notification},
    },
    codec::JsonRpcCodec,
    error::{Error, Result},
    message::{self, SubscriptionID},
};

/// Build the QUIC backend by dialing the server.
pub(super) async fn build_backend(config: &ClientConfig) -> Result<ClientBackend> {
    let quic_config = config
        .quic_config
        .clone()
        .ok_or(Error::QUICConfigRequired)?;
    let quic_conn = QuicEndpoint::dial(&config.endpoint, quic_config).await?;

    info!(
        "Successfully connected to the RPC server: {:?}",
        quic_conn.peer_endpoint().ok()
    );

    Ok(ClientBackend::QuicStream {
        quic_conn: Arc::new(quic_conn),
        subscriptions: Subscriptions::new(config.subscription_buffer_size),
    })
}

pub(super) async fn call<B, W, T>(
    client: &Client<B, W>,
    quic_conn: &QuicConn,
    method: &str,
    params: T,
) -> Result<message::Response>
where
    B: JsonRpcCodec,
    W: WsCodec,
    T: Serialize + DeserializeOwned,
{
    if client.disconnect.load(Ordering::Relaxed) {
        return Err(Error::ClientDisconnected);
    }

    let stream = quic_conn.open_stream().await?;
    let mut conn = framed(stream, client.codec.clone());

    let id: RequestID = random_32();
    let request = message::Request {
        jsonrpc: message::JSONRPC_VERSION.to_string(),
        id: json!(id),
        method: method.to_string(),
        params: Some(json!(params)),
    };

    conn.send_msg(serde_json::to_value(request)?).await?;
    let msg = conn.recv_msg().await?;
    let response: message::Response = serde_json::from_value(msg)?;
    Ok(response)
}

pub(super) async fn subscribe<B, W, T>(
    client: &Arc<Client<B, W>>,
    quic_conn: &QuicConn,
    subscriptions: &Arc<Subscriptions>,
    method: &str,
    params: T,
) -> Result<Arc<Subscription>>
where
    B: JsonRpcCodec,
    W: WsCodec,
    T: Serialize + DeserializeOwned,
{
    if client.disconnect.load(Ordering::Relaxed) {
        return Err(Error::ClientDisconnected);
    }

    let stream = quic_conn.open_stream().await?;
    let mut conn = framed(stream, client.codec.clone());

    let id: RequestID = random_32();
    let request = message::Request {
        jsonrpc: message::JSONRPC_VERSION.to_string(),
        id: json!(id),
        method: method.to_string(),
        params: Some(json!(params)),
    };

    conn.send_msg(serde_json::to_value(request)?).await?;

    let msg = conn.recv_msg().await?;
    let response: message::Response = serde_json::from_value(msg)?;

    if let Some(error) = response.error {
        return Err(Error::SubscribeError(error.code, error.message));
    }

    let sub_id = match response.result {
        Some(result) => serde_json::from_value::<SubscriptionID>(result)?,
        None => return Err(Error::InvalidMsg("Invalid subscription id".to_string())),
    };

    let (sub, unsub_rx) = subscriptions.subscribe_stream(sub_id).await;
    let (reader, writer) = conn.split();

    client.task_group.spawn_then(
        subscription_stream_task(
            reader,
            writer,
            unsub_rx,
            sub.clone(),
            subscriptions.clone(),
            client.config.notification_decoder,
        ),
        |res: TaskResult<Result<()>>| async move {
            debug!("QUIC subscription stream task ended: {res}");
        },
    );

    Ok(sub)
}

/// Forward notifications until the stream ends or an unsubscribe request
/// arrives. The request goes out on the same stream. Removes the
/// subscription when it exits.
async fn subscription_stream_task<B: JsonRpcCodec>(
    mut reader: FramedReader<B>,
    mut writer: FramedWriter<B>,
    unsub_rx: Receiver<Value>,
    sub: Arc<Subscription>,
    subscriptions: Arc<Subscriptions>,
    decoder: NotificationDecoder,
) -> Result<()> {
    let unsub_msg = loop {
        match select(reader.recv_msg(), unsub_rx.recv()).await {
            Either::Left(Ok(msg)) => {
                let nt_res = match decode_notification(decoder, msg) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let val = nt_res.result.unwrap_or(json!(""));
                if sub.notify(val).await.is_err() {
                    // Closed by `unsubscribe()`: its request may be waiting.
                    break unsub_rx.try_recv().ok();
                }
            }
            Either::Left(Err(_)) => break None,
            Either::Right(msg) => break msg.ok(),
        }
    };
    if let Some(msg) = unsub_msg {
        let _ = writer.send_msg(msg).await;
    }
    // Remove the subscription, so `recv()` ends when the stream ends.
    subscriptions.unsubscribe(&sub.id()).await;
    Ok(())
}
