use std::collections::HashMap;

use async_channel::{Receiver, Sender};

use karyon_core::async_runtime::lock::Mutex;

use crate::{message, Error, Result};

use super::RequestID;

const CHANNEL_CAP: usize = 10;

/// Manages client requests
pub(super) struct MessageDispatcher {
    chans: Mutex<HashMap<RequestID, Sender<message::Response>>>,
}

impl MessageDispatcher {
    /// Creates a new MessageDispatcher
    pub(super) fn new() -> Self {
        Self {
            chans: Mutex::new(HashMap::new()),
        }
    }

    /// Registers a new request with a given ID and returns a Receiver channel
    /// to wait for the response.
    pub(super) async fn register(&self, id: RequestID) -> Receiver<message::Response> {
        let (tx, rx) = async_channel::bounded(CHANNEL_CAP);
        self.chans.lock().await.insert(id, tx);
        rx
    }

    /// Unregisters the request with the provided ID
    pub(super) async fn unregister(&self, id: &RequestID) {
        self.chans.lock().await.remove(id);
    }

    /// Dispatches a response to the channel associated with the response's ID.
    ///
    /// If a channel is registered for the response's ID, the response is sent
    /// through that channel. If no channel is found for the ID, returns an error.
    pub(super) async fn dispatch(&self, res: message::Response) -> Result<()> {
        if res.id.is_none() {
            return Err(Error::InvalidMsg("Response id is none"));
        }
        let id: RequestID = serde_json::from_value(res.id.clone().unwrap())?;
        let val = self.chans.lock().await.remove(&id);
        match val {
            Some(tx) => tx.send(res).await.map_err(Error::from),
            None => Err(Error::InvalidMsg("Receive unknown message")),
        }
    }
}