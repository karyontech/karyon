use std::{collections::HashMap, sync::Arc};

use futures_util::stream::{FuturesUnordered, StreamExt};
use log::error;

use crate::{async_runtime::lock::Mutex, util::random_32, Result};

const CHANNEL_BUFFER_SIZE: usize = 1000;

pub type ArcPublisher<T> = Arc<Publisher<T>>;
pub type SubscriptionID = u32;

/// A simple publish-subscribe system.
// # Example
///
/// ```
/// use karyon_core::pubsub::{Publisher};
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
    subs: Mutex<HashMap<SubscriptionID, async_channel::Sender<T>>>,
    subscription_buffer_size: usize,
}

impl<T: Clone> Publisher<T> {
    /// Creates a new [`Publisher`]
    pub fn new() -> ArcPublisher<T> {
        Arc::new(Self {
            subs: Mutex::new(HashMap::new()),
            subscription_buffer_size: CHANNEL_BUFFER_SIZE,
        })
    }

    /// Creates a new [`Publisher`] with the provided buffer size for the
    /// [`Subscription`] channel.
    ///
    /// This is important to control the memory used by the [`Subscription`] channel.
    /// If the subscriber can't keep up with the new messages coming, then the
    /// channel buffer will fill with new messages, and if the buffer is full,
    /// the emit function will block until the subscriber starts to process
    /// the buffered messages.
    ///
    /// If `size` is zero, this function will panic.
    pub fn with_buffer_size(size: usize) -> ArcPublisher<T> {
        Arc::new(Self {
            subs: Mutex::new(HashMap::new()),
            subscription_buffer_size: size,
        })
    }

    /// Subscribes and return a [`Subscription`]
    pub async fn subscribe(self: &Arc<Self>) -> Subscription<T> {
        let mut subs = self.subs.lock().await;

        let chan = async_channel::bounded(self.subscription_buffer_size);

        let mut sub_id = random_32();

        // Generate a new one if sub_id already exists
        while subs.contains_key(&sub_id) {
            sub_id = random_32();
        }

        let sub = Subscription::new(sub_id, self.clone(), chan.1);
        subs.insert(sub_id, chan.0);

        sub
    }

    /// Unsubscribes from the publisher
    pub async fn unsubscribe(self: &Arc<Self>, id: &SubscriptionID) {
        self.subs.lock().await.remove(id);
    }

    /// Notifies all subscribers
    pub async fn notify(self: &Arc<Self>, value: &T) {
        let mut subs = self.subs.lock().await;

        let mut results = FuturesUnordered::new();
        let mut closed_subs = vec![];

        for (sub_id, sub) in subs.iter() {
            let result = async { (*sub_id, sub.send(value.clone()).await) };
            results.push(result);
        }

        while let Some((id, fut_err)) = results.next().await {
            if let Err(err) = fut_err {
                error!("failed to notify {}: {}", id, err);
                closed_subs.push(id);
            }
        }
        drop(results);

        for sub_id in closed_subs.iter() {
            subs.remove(sub_id);
        }
    }
}

// Subscription
pub struct Subscription<T> {
    id: SubscriptionID,
    recv_chan: async_channel::Receiver<T>,
    publisher: ArcPublisher<T>,
}

impl<T: Clone> Subscription<T> {
    /// Creates a new Subscription
    pub fn new(
        id: SubscriptionID,
        publisher: ArcPublisher<T>,
        recv_chan: async_channel::Receiver<T>,
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
