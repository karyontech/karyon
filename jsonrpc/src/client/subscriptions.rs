use std::{collections::HashMap, sync::Arc};

use async_channel::{Receiver, Sender, bounded};
#[cfg(feature = "quic")]
use log::debug;
use serde_json::{Value, from_value, json};

use karyon_core::async_runtime::lock::RwLock;

use crate::{
    error::{Error, Result},
    message::{Notification, NotificationResult, SubscriptionID},
};

/// Turns a received notification into its subscription id and result.
/// Must match the server's notification encoder.
pub type NotificationDecoder = fn(Notification) -> Result<NotificationResult>;

/// Default decoder: `params` is `{"subscription": id, "result": value}`.
pub(super) fn default_notification_decoder(nt: Notification) -> Result<NotificationResult> {
    match nt.params {
        Some(p) => Ok(from_value(p)?),
        None => Err(Error::InvalidMsg("Invalid notification msg".to_string())),
    }
}

/// Parse a raw notification message and decode it with `decoder`.
#[cfg(feature = "quic")]
pub(super) fn decode_notification(
    decoder: NotificationDecoder,
    msg: Value,
) -> Result<NotificationResult> {
    let nt: Notification = from_value(msg)?;
    debug!("<-- {nt}");
    decoder(nt)
}

/// A subscription established when the client's subscribe to a method
pub struct Subscription {
    id: SubscriptionID,
    rx: Receiver<Value>,
    tx: Sender<Value>,
    /// Set when the subscription has its own stream (QUIC, HTTP/3). Hands
    /// the unsubscribe request to the task that owns that stream.
    #[cfg(feature = "quic")]
    unsub: Option<Sender<Value>>,
}

/// Shared handle to a `Subscription`.
pub type SubscriptionRef = Arc<Subscription>;

impl Subscription {
    fn new(id: SubscriptionID, buffer_size: usize) -> Self {
        let (tx, rx) = bounded(buffer_size);
        Self {
            tx,
            id,
            rx,
            #[cfg(feature = "quic")]
            unsub: None,
        }
    }

    pub async fn recv(&self) -> Result<Value> {
        self.rx.recv().await.map_err(|_| Error::SubscriptionClosed)
    }

    pub fn id(&self) -> SubscriptionID {
        self.id
    }

    pub(crate) async fn notify(&self, val: Value) -> Result<()> {
        if self.tx.is_full() {
            return Err(Error::SubscriptionBufferFull);
        }
        self.tx.send(val).await?;
        Ok(())
    }

    fn close(&self) {
        self.tx.close();
    }
}

/// Manages subscriptions for the client.
pub(crate) struct Subscriptions {
    subs: RwLock<HashMap<SubscriptionID, SubscriptionRef>>,
    sub_buffer_size: usize,
}

/// Shared handle to the `Subscriptions`.
pub(crate) type SubscriptionsRef = Arc<Subscriptions>;

impl Subscriptions {
    /// Creates a new [`Subscriptions`].
    pub(super) fn new(sub_buffer_size: usize) -> SubscriptionsRef {
        Arc::new(Self {
            subs: RwLock::new(HashMap::new()),
            sub_buffer_size,
        })
    }

    /// Returns a new [`Subscription`]
    pub(super) async fn subscribe(&self, id: SubscriptionID) -> SubscriptionRef {
        let sub = Arc::new(Subscription::new(id, self.sub_buffer_size));
        self.subs.write().await.insert(id, sub.clone());
        sub
    }

    /// Returns a new [`Subscription`] that has its own stream, and the
    /// receiver its stream task reads the unsubscribe request from.
    #[cfg(feature = "quic")]
    pub(super) async fn subscribe_stream(
        &self,
        id: SubscriptionID,
    ) -> (SubscriptionRef, Receiver<Value>) {
        let (unsub_tx, unsub_rx) = bounded(1);
        let mut sub = Subscription::new(id, self.sub_buffer_size);
        sub.unsub = Some(unsub_tx);
        let sub = Arc::new(sub);
        self.subs.write().await.insert(id, sub.clone());
        (sub, unsub_rx)
    }

    /// Closes subscription channels and clear the inner map.
    pub(super) async fn clear(&self) {
        let mut subs = self.subs.write().await;
        for sub in subs.values() {
            sub.close();
        }
        subs.clear();
    }

    /// Unsubscribe from the provided subscription id.
    pub(super) async fn unsubscribe(&self, id: &SubscriptionID) {
        if let Some(sub) = self.subs.write().await.remove(id) {
            sub.close();
        }
    }

    /// Unsubscribe from a subscription that has its own stream. `msg` is
    /// the unsubscribe request; the stream task sends it.
    #[cfg(feature = "quic")]
    pub(super) async fn unsubscribe_stream(&self, id: &SubscriptionID, msg: Value) {
        let Some(sub) = self.subs.write().await.remove(id) else {
            return;
        };
        // Hand over the request before closing: the task stops once the
        // subscription is closed.
        if let Some(unsub) = &sub.unsub {
            let _ = unsub.send(msg).await;
        }
        sub.close();
    }

    /// Notifies the subscription about the given decoded notification.
    pub(super) async fn notify(&self, nt_res: NotificationResult) -> Result<()> {
        match self.subs.read().await.get(&nt_res.subscription) {
            Some(s) => s.notify(nt_res.result.unwrap_or(json!(""))).await?,
            None => {
                return Err(Error::InvalidMsg("Unknown notification".to_string()));
            }
        }

        Ok(())
    }
}
