use std::{collections::HashMap, sync::Arc};

use async_channel::{Receiver, Sender};
use serde_json::json;
use serde_json::Value;

use karyon_core::async_runtime::lock::Mutex;

use crate::{
    error::{Error, Result},
    message::{Notification, NotificationResult, SubscriptionID},
};

/// A subscription established when the client's subscribe to a method
pub struct Subscription {
    id: SubscriptionID,
    rx: Receiver<Value>,
    tx: Sender<Value>,
}

impl Subscription {
    fn new(id: SubscriptionID, buffer_size: usize) -> Arc<Self> {
        let (tx, rx) = async_channel::bounded(buffer_size);
        Arc::new(Self { tx, id, rx })
    }

    pub async fn recv(&self) -> Result<Value> {
        self.rx.recv().await.map_err(|_| Error::SubscriptionClosed)
    }

    pub fn id(&self) -> SubscriptionID {
        self.id
    }

    async fn notify(&self, val: Value) -> Result<()> {
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
pub(super) struct Subscriptions {
    subs: Mutex<HashMap<SubscriptionID, Arc<Subscription>>>,
    sub_buffer_size: usize,
}

impl Subscriptions {
    /// Creates a new [`Subscriptions`].
    pub(super) fn new(sub_buffer_size: usize) -> Arc<Self> {
        Arc::new(Self {
            subs: Mutex::new(HashMap::new()),
            sub_buffer_size,
        })
    }

    /// Returns a new [`Subscription`]
    pub(super) async fn subscribe(&self, id: SubscriptionID) -> Arc<Subscription> {
        let sub = Subscription::new(id, self.sub_buffer_size);
        self.subs.lock().await.insert(id, sub.clone());
        sub
    }

    /// Closes subscription channels and clear the inner map.
    pub(super) async fn clear(&self) {
        let mut subs = self.subs.lock().await;
        for (_, sub) in subs.iter() {
            sub.close();
        }
        subs.clear();
    }

    /// Unsubscribe from the provided subscription id.
    pub(super) async fn unsubscribe(&self, id: &SubscriptionID) {
        if let Some(sub) = self.subs.lock().await.remove(id) {
            sub.close();
        }
    }

    /// Notifies the subscription about the given notification.
    pub(super) async fn notify(&self, nt: Notification) -> Result<()> {
        let nt_res: NotificationResult = match nt.params {
            Some(ref p) => serde_json::from_value(p.clone())?,
            None => return Err(Error::InvalidMsg("Invalid notification msg".to_string())),
        };

        match self.subs.lock().await.get(&nt_res.subscription) {
            Some(s) => s.notify(nt_res.result.unwrap_or(json!(""))).await?,
            None => {
                return Err(Error::InvalidMsg("Unknown notification".to_string()));
            }
        }

        Ok(())
    }
}
