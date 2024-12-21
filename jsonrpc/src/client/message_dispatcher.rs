use std::collections::HashMap;

use async_channel::{Receiver, Sender};

use karyon_core::async_runtime::lock::Mutex;

use crate::{
    error::{Error, Result},
    message,
};

use super::RequestID;

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
        let (tx, rx) = async_channel::bounded(1);
        self.chans.lock().await.insert(id, tx);
        rx
    }

    /// Unregisters the request with the provided ID
    pub(super) async fn unregister(&self, id: &RequestID) {
        self.chans.lock().await.remove(id);
    }

    /// Clear the registered channels.
    pub(super) async fn clear(&self) {
        let mut chans = self.chans.lock().await;
        for (_, tx) in chans.iter() {
            tx.close();
        }
        chans.clear();
    }

    /// Dispatches a response to the channel associated with the response's ID.
    ///
    /// If a channel is registered for the response's ID, the response is sent
    /// through that channel. If no channel is found for the ID, returns an error.
    pub(super) async fn dispatch(&self, res: message::Response) -> Result<()> {
        let res_id = match res.id {
            Some(ref rid) => rid.clone(),
            None => {
                return Err(Error::InvalidMsg("Response id is none"));
            }
        };
        let id: RequestID = serde_json::from_value(res_id)?;
        let val = self.chans.lock().await.remove(&id);
        match val {
            Some(tx) => tx.send(res).await.map_err(Error::from),
            None => Err(Error::InvalidMsg("Receive unknown message")),
        }
    }
}
