use std::{collections::HashMap, sync::Arc};

use log::error;
use smol::lock::Mutex;

use crate::{utils::random_16, Result};

pub type ArcPublisher<T> = Arc<Publisher<T>>;
pub type SubscriptionID = u16;

/// A simple publish-subscribe system.
// # Example
///
/// ```
/// use karyons_core::pubsub::{Publisher};
///
///  async {
///     let publisher = Publisher::new();
///     
///     let sub = publisher.subscribe().await;
///     
///     publisher.notify(&String::from("MESSAGE")).await;
///
///     let msg = sub.recv().await;
///
///     // ....
///  };
///  
/// ```
pub struct Publisher<T> {
    subs: Mutex<HashMap<SubscriptionID, smol::channel::Sender<T>>>,
}

impl<T: Clone> Publisher<T> {
    /// Creates a new Publisher
    pub fn new() -> ArcPublisher<T> {
        Arc::new(Self {
            subs: Mutex::new(HashMap::new()),
        })
    }

    /// Subscribe and return a Subscription
    pub async fn subscribe(self: &Arc<Self>) -> Subscription<T> {
        let mut subs = self.subs.lock().await;

        let chan = smol::channel::unbounded();

        let mut sub_id = random_16();

        // While the SubscriptionID already exists, generate a new one
        while subs.contains_key(&sub_id) {
            sub_id = random_16();
        }

        let sub = Subscription::new(sub_id, self.clone(), chan.1);
        subs.insert(sub_id, chan.0);

        sub
    }

    /// Unsubscribe from the Publisher
    pub async fn unsubscribe(self: &Arc<Self>, id: &SubscriptionID) {
        self.subs.lock().await.remove(id);
    }

    /// Notify all subscribers
    pub async fn notify(self: &Arc<Self>, value: &T) {
        let mut subs = self.subs.lock().await;
        let mut closed_subs = vec![];

        for (sub_id, sub) in subs.iter() {
            if let Err(err) = sub.send(value.clone()).await {
                error!("failed to notify {}: {}", sub_id, err);
                closed_subs.push(*sub_id);
            }
        }

        for sub_id in closed_subs.iter() {
            subs.remove(sub_id);
        }
    }
}

// Subscription
pub struct Subscription<T> {
    id: SubscriptionID,
    recv_chan: smol::channel::Receiver<T>,
    publisher: ArcPublisher<T>,
}

impl<T: Clone> Subscription<T> {
    /// Creates a new Subscription
    pub fn new(
        id: SubscriptionID,
        publisher: ArcPublisher<T>,
        recv_chan: smol::channel::Receiver<T>,
    ) -> Subscription<T> {
        Self {
            id,
            recv_chan,
            publisher,
        }
    }

    /// Receive a message from the Publisher
    pub async fn recv(&self) -> Result<T> {
        let msg = self.recv_chan.recv().await?;
        Ok(msg)
    }

    /// Unsubscribe from the Publisher
    pub async fn unsubscribe(&self) {
        self.publisher.unsubscribe(&self.id).await;
    }
}
