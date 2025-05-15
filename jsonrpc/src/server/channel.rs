use std::collections::HashSet;
use std::sync::{Arc, Weak};

use karyon_core::{async_runtime::lock::Mutex, util::random_32};

use crate::{
    error::{Error, Result},
    message::SubscriptionID,
};

#[derive(Debug)]
pub struct NewNotification {
    pub sub_id: SubscriptionID,
    pub result: serde_json::Value,
    pub method: String,
}

/// Represents a new subscription
#[derive(Clone)]
pub struct Subscription {
    pub id: SubscriptionID,
    parent: Weak<Channel>,
    chan: async_channel::Sender<NewNotification>,
    method: String,
}

impl Subscription {
    /// Creates a new [`Subscription`]
    fn new(
        parent: Weak<Channel>,
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
        if self.still_subscribed().await {
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

    /// Checks from the partent if this subscription is still subscribed
    async fn still_subscribed(&self) -> bool {
        match self.parent.upgrade() {
            Some(parent) => parent.subs.lock().await.contains(&self.id),
            None => false,
        }
    }
}

/// Represents a connection channel for creating/removing subscriptions
pub struct Channel {
    chan: async_channel::Sender<NewNotification>,
    subs: Mutex<HashSet<SubscriptionID>>,
}

impl Channel {
    /// Creates a new [`Channel`]
    pub(crate) fn new(chan: async_channel::Sender<NewNotification>) -> Arc<Channel> {
        Arc::new(Self {
            chan,
            subs: Mutex::new(HashSet::new()),
        })
    }

    /// Creates a new [`Subscription`]
    pub async fn new_subscription(
        self: &Arc<Self>,
        method: &str,
        sub_id: Option<SubscriptionID>,
    ) -> Result<Subscription> {
        let sub_id = sub_id.unwrap_or_else(random_32);
        if !self.subs.lock().await.insert(sub_id) {
            return Err(Error::SubscriptionDuplicated(sub_id.to_string()));
        }

        let sub = Subscription::new(Arc::downgrade(self), sub_id, self.chan.clone(), method);
        Ok(sub)
    }

    /// Removes a [`Subscription`]
    pub async fn remove_subscription(&self, id: &SubscriptionID) -> Result<()> {
        let mut subs = self.subs.lock().await;
        if !subs.remove(id) {
            return Err(Error::SubscriptionNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Closes the [`Channel`]
    pub(crate) fn close(&self) {
        self.chan.close();
    }
}
