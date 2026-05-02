//! QUIC client mode: one stream per call, one stream per subscription.

use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc},
};

use async_channel::Sender;
use log::{debug, info};
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

use karyon_core::{
    async_util::{select, Either, TaskResult},
    util::random_32,
};

use karyon_net::{
    framed,
    quic::{QuicConn, QuicEndpoint},
    FramedReader, FramedWriter, StreamMux,
};

use crate::{
    client::{
        subscriptions::{Subscription, Subscriptions},
        Client, ClientBackend, ClientConfig, RequestID, WsCodec,
    },
    codec::{JsonCodec, JsonRpcCodec},
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
        unsub_chans: Mutex::new(HashMap::new()),
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
    let mut conn = framed(stream, JsonCodec::default());

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
    subscriptions: &Subscriptions,
    unsub_chans: &Mutex<HashMap<SubscriptionID, Sender<serde_json::Value>>>,
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
    let mut conn = framed(stream, JsonCodec::default());

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

    let (unsub_tx, unsub_rx) = async_channel::bounded::<serde_json::Value>(1);
    unsub_chans.lock().insert(sub_id, unsub_tx);

    let sub = subscriptions.subscribe(sub_id).await;

    let (reader, writer) = conn.split();

    client.task_group.spawn(
        subscription_stream_task(reader, writer, unsub_rx, sub.clone()),
        |res: TaskResult<Result<()>>| async move {
            debug!("QUIC subscription stream task ended: {res}");
        },
    );

    Ok(sub)
}

async fn subscription_stream_task(
    mut reader: FramedReader<JsonCodec>,
    mut writer: FramedWriter<JsonCodec>,
    unsub_rx: async_channel::Receiver<serde_json::Value>,
    sub: Arc<Subscription>,
) -> Result<()> {
    loop {
        match select(reader.recv_msg(), unsub_rx.recv()).await {
            Either::Left(msg) => {
                let msg = match msg {
                    Ok(m) => m,
                    Err(_) => break,
                };
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
            Either::Right(unsub_msg) => {
                if let Ok(msg) = unsub_msg {
                    let _ = writer.send_msg(msg).await;
                }
                break;
            }
        }
    }
    Ok(())
}
