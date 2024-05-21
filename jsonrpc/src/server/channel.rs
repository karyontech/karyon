use std::sync::Arc;

use karyon_core::{async_runtime::lock::Mutex, util::random_32};

use crate::{Error, Result};

pub type SubscriptionID = u32;
pub type ArcChannel = Arc<Channel>;

/// Represents a new subscription
pub struct Subscription {
    pub id: SubscriptionID,
    parent: Arc<Channel>,
    chan: async_channel::Sender<(SubscriptionID, serde_json::Value)>,
}

impl Subscription {
    /// Creates a new `Subscription`
    fn new(
        parent: Arc<Channel>,
        id: SubscriptionID,
        chan: async_channel::Sender<(SubscriptionID, serde_json::Value)>,
    ) -> Self {
        Self { parent, id, chan }
    }

    /// Sends a notification to the subscriber
    pub async fn notify(&self, res: serde_json::Value) -> Result<()> {
        if self.parent.subs.lock().await.contains(&self.id) {
            self.chan.send((self.id, res)).await?;
            Ok(())
        } else {
            Err(Error::SubscriptionNotFound(self.id.to_string()))
        }
    }
}

/// Represents a channel for creating/removing subscriptions
pub struct Channel {
    chan: async_channel::Sender<(SubscriptionID, serde_json::Value)>,
    subs: Mutex<Vec<SubscriptionID>>,
}

impl Channel {
    /// Creates a new `Channel`
    pub fn new(chan: async_channel::Sender<(SubscriptionID, serde_json::Value)>) -> ArcChannel {
        Arc::new(Self {
            chan,
            subs: Mutex::new(Vec::new()),
        })
    }

    /// Creates a new subscription
    pub async fn new_subscription(self: &Arc<Self>) -> Subscription {
        let sub_id = random_32();
        let sub = Subscription::new(self.clone(), sub_id, self.chan.clone());
        self.subs.lock().await.push(sub_id);
        sub
    }

    /// Removes a subscription
    pub async fn remove_subscription(self: &Arc<Self>, id: &SubscriptionID) {
        let i = match self.subs.lock().await.iter().position(|i| i == id) {
            Some(i) => i,
            None => return,
        };
        self.subs.lock().await.remove(i);
    }
}
