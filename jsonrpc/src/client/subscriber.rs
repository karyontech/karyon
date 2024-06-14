use std::collections::HashMap;

use async_channel::{Receiver, Sender};
use log::warn;
use serde_json::json;

use karyon_core::async_runtime::lock::Mutex;

use crate::{
    message::{Notification, NotificationResult, SubscriptionID},
    Error, Result,
};

/// Manages subscriptions for the client.
pub(super) struct Subscriber {
    subs: Mutex<HashMap<SubscriptionID, Sender<serde_json::Value>>>,
}

/// Type alias for a subscription to receive notifications.
///
/// The receiver channel is returned by the `subscribe`
pub type Subscription = Receiver<serde_json::Value>;

impl Subscriber {
    pub(super) fn new() -> Self {
        Self {
            subs: Mutex::new(HashMap::new()),
        }
    }

    pub(super) async fn subscribe(&self, id: SubscriptionID) -> Receiver<serde_json::Value> {
        let (ch_tx, ch_rx) = async_channel::unbounded();
        self.subs.lock().await.insert(id, ch_tx);
        ch_rx
    }

    pub(super) async fn drop_all(&self) {
        self.subs.lock().await.clear();
    }

    /// Unsubscribe
    pub(super) async fn unsubscribe(&self, id: &SubscriptionID) {
        self.subs.lock().await.remove(id);
    }

    pub(super) async fn notify(&self, nt: Notification) -> Result<()> {
        let nt_res: NotificationResult = match nt.params {
            Some(ref p) => serde_json::from_value(p.clone())?,
            None => return Err(Error::InvalidMsg("Invalid notification msg")),
        };
        match self.subs.lock().await.get(&nt_res.subscription) {
            Some(s) => {
                s.send(nt_res.result.unwrap_or(json!(""))).await?;
                Ok(())
            }
            None => {
                warn!("Receive unknown notification {}", nt_res.subscription);
                Ok(())
            }
        }
    }
}
