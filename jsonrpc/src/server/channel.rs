use std::sync::Arc;

use karyon_core::{async_runtime::lock::Mutex, util::random_32};

use crate::{message::SubscriptionID, Error, Result};

pub type ArcChannel = Arc<Channel>;

pub(crate) struct NewNotification {
    pub sub_id: SubscriptionID,
    pub result: serde_json::Value,
    pub method: String,
}

/// Represents a new subscription
pub struct Subscription {
    pub id: SubscriptionID,
    parent: Arc<Channel>,
    chan: async_channel::Sender<NewNotification>,
    method: String,
}

impl Subscription {
    /// Creates a new `Subscription`
    fn new(
        parent: Arc<Channel>,
        id: SubscriptionID,
        chan: async_channel::Sender<NewNotification>,
        method: &str,
    ) -> Self {
        Self {
            parent,
            id,
            chan,
            method: method.to_string(),
        }
    }

    /// Sends a notification to the subscriber
    pub async fn notify(&self, res: serde_json::Value) -> Result<()> {
        if self.parent.subs.lock().await.contains(&self.id) {
            let nt = NewNotification {
                sub_id: self.id,
                result: res,
                method: self.method.clone(),
            };
            self.chan.send(nt).await?;
            Ok(())
        } else {
            Err(Error::SubscriptionNotFound(self.id.to_string()))
        }
    }
}

/// Represents a channel for creating/removing subscriptions
pub struct Channel {
    chan: async_channel::Sender<NewNotification>,
    subs: Mutex<Vec<SubscriptionID>>,
}

impl Channel {
    /// Creates a new `Channel`
    pub(crate) fn new(chan: async_channel::Sender<NewNotification>) -> ArcChannel {
        Arc::new(Self {
            chan,
            subs: Mutex::new(Vec::new()),
        })
    }

    /// Creates a new subscription
    pub async fn new_subscription(self: &Arc<Self>, method: &str) -> Subscription {
        let sub_id = random_32();
        let sub = Subscription::new(self.clone(), sub_id, self.chan.clone(), method);
        self.subs.lock().await.push(sub_id);
        sub
    }

    /// Removes a subscription
    pub async fn remove_subscription(self: &Arc<Self>, id: &SubscriptionID) {
        let mut subs = self.subs.lock().await;
        let i = match subs.iter().position(|i| i == id) {
            Some(i) => i,
            None => return,
        };
        subs.remove(i);
    }
}
