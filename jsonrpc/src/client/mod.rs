pub mod builder;

use std::{collections::HashMap, sync::Arc, time::Duration};

use log::{debug, error, warn};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

use karyon_core::{
    async_runtime::lock::Mutex,
    async_util::{timeout, TaskGroup, TaskResult},
    util::random_32,
};
use karyon_net::Conn;

use crate::{
    message::{self, SubscriptionID},
    Error, Result,
};

const CHANNEL_CAP: usize = 10;

/// Type alias for a subscription to receive notifications.
///
/// The receiver channel is returned by the `subscribe` method to receive
/// notifications from the server.
pub type Subscription = async_channel::Receiver<serde_json::Value>;

/// Represents an RPC client
pub struct Client {
    conn: Conn<serde_json::Value>,
    timeout: Option<u64>,
    chans: Mutex<HashMap<u32, async_channel::Sender<message::Response>>>,
    subscriptions: Mutex<HashMap<SubscriptionID, async_channel::Sender<serde_json::Value>>>,
    task_group: TaskGroup,
}

impl Client {
    /// Calls the provided method, waits for the response, and returns the result.
    pub async fn call<T: Serialize + DeserializeOwned, V: DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<V> {
        let response = self.send_request(method, params).await?;

        match response.result {
            Some(result) => Ok(serde_json::from_value::<V>(result)?),
            None => Err(Error::InvalidMsg("Invalid response result")),
        }
    }

    /// Subscribes to the provided method, waits for the response, and returns the result.
    ///
    /// This function sends a subscription request to the specified method
    /// with the given parameters. It waits for the response and returns a
    /// tuple containing a `SubscriptionID` and a `Subscription` (channel receiver).
    pub async fn subscribe<T: Serialize + DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<(SubscriptionID, Subscription)> {
        let response = self.send_request(method, params).await?;

        let sub_id = match response.result {
            Some(result) => serde_json::from_value::<SubscriptionID>(result)?,
            None => return Err(Error::InvalidMsg("Invalid subscription id")),
        };

        let (ch_tx, ch_rx) = async_channel::bounded(CHANNEL_CAP);
        self.subscriptions.lock().await.insert(sub_id, ch_tx);

        Ok((sub_id, ch_rx))
    }

    /// Unsubscribes from the provided method, waits for the response, and returns the result.
    ///
    /// This function sends an unsubscription request for the specified method
    /// and subscription ID. It waits for the response to confirm the unsubscription.
    pub async fn unsubscribe(&self, method: &str, sub_id: SubscriptionID) -> Result<()> {
        let _ = self.send_request(method, sub_id).await?;
        self.subscriptions.lock().await.remove(&sub_id);
        Ok(())
    }

    async fn send_request<T: Serialize + DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<message::Response> {
        let id = random_32();
        let request = message::Request {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            id: json!(id),
            method: method.to_string(),
            params: Some(json!(params)),
        };

        let req_json = serde_json::to_value(&request)?;

        self.conn.send(req_json).await?;

        let (tx, rx) = async_channel::bounded(CHANNEL_CAP);
        self.chans.lock().await.insert(id, tx);

        let response = match self.wait_for_response(rx).await {
            Ok(r) => r,
            Err(err) => {
                self.chans.lock().await.remove(&id);
                return Err(err);
            }
        };

        if let Some(error) = response.error {
            return Err(Error::SubscribeError(error.code, error.message));
        }

        if *response.id.as_ref().unwrap() != request.id {
            return Err(Error::InvalidMsg("Invalid response id"));
        }

        debug!("--> {request}");
        Ok(response)
    }

    async fn wait_for_response(
        &self,
        rx: async_channel::Receiver<message::Response>,
    ) -> Result<message::Response> {
        match self.timeout {
            Some(t) => timeout(Duration::from_millis(t), rx.recv())
                .await?
                .map_err(Error::from),
            None => rx.recv().await.map_err(Error::from),
        }
    }

    fn start_background_receiving(self: &Arc<Self>) {
        let selfc = self.clone();
        let on_failure = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("background receiving stopped: {err}");
            }
            // drop all subscription channels
            selfc.subscriptions.lock().await.clear();
        };
        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                loop {
                    let msg = selfc.conn.recv().await?;
                    selfc.handle_msg(msg).await?;
                }
            },
            on_failure,
        );
    }

    async fn handle_msg(&self, msg: serde_json::Value) -> Result<()> {
        if let Ok(res) = serde_json::from_value::<message::Response>(msg.clone()) {
            debug!("<-- {res}");
            if res.id.is_none() {
                return Err(Error::InvalidMsg("Response id is none"));
            }

            let id: u32 = serde_json::from_value(res.id.clone().unwrap())?;
            match self.chans.lock().await.remove(&id) {
                Some(tx) => tx.send(res).await?,
                None => return Err(Error::InvalidMsg("Receive unkown message")),
            }

            return Ok(());
        }

        if let Ok(nt) = serde_json::from_value::<message::Notification>(msg.clone()) {
            debug!("<-- {nt}");
            let sub_result: message::NotificationResult = match nt.params {
                Some(ref p) => serde_json::from_value(p.clone())?,
                None => return Err(Error::InvalidMsg("Invalid notification msg")),
            };

            match self
                .subscriptions
                .lock()
                .await
                .get(&sub_result.subscription)
            {
                Some(s) => {
                    s.send(sub_result.result.unwrap_or(json!(""))).await?;
                    return Ok(());
                }
                None => {
                    warn!("Receive unknown notification {}", sub_result.subscription);
                    return Ok(());
                }
            }
        }

        error!("Receive unexpected msg: {msg}");
        Err(Error::InvalidMsg("Unexpected msg"))
    }
}
