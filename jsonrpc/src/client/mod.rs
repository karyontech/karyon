pub mod builder;
mod message_dispatcher;
mod subscriptions;

use std::{sync::Arc, time::Duration};

use log::{debug, error};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;

use karyon_core::{
    async_util::{timeout, TaskGroup, TaskResult},
    util::random_32,
};
use karyon_net::Conn;

use crate::{
    message::{self, SubscriptionID},
    Error, Result,
};

use message_dispatcher::MessageDispatcher;
pub use subscriptions::Subscription;
use subscriptions::Subscriptions;

type RequestID = u32;

/// Represents an RPC client
pub struct Client {
    conn: Conn<serde_json::Value>,
    timeout: Option<u64>,
    message_dispatcher: MessageDispatcher,
    task_group: TaskGroup,
    subscriptions: Arc<Subscriptions>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum NewMsg {
    Notification(message::Notification),
    Response(message::Response),
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

        let sub = self.subscriptions.subscribe(sub_id).await;

        Ok((sub_id, sub))
    }

    /// Unsubscribes from the provided method, waits for the response, and returns the result.
    ///
    /// This function sends an unsubscription request for the specified method
    /// and subscription ID. It waits for the response to confirm the unsubscription.
    pub async fn unsubscribe(&self, method: &str, sub_id: SubscriptionID) -> Result<()> {
        let _ = self.send_request(method, sub_id).await?;
        self.subscriptions.unsubscribe(&sub_id).await;
        Ok(())
    }

    async fn send_request<T: Serialize + DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<message::Response> {
        let id: RequestID = random_32();
        let request = message::Request {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            id: json!(id),
            method: method.to_string(),
            params: Some(json!(params)),
        };

        let req_json = serde_json::to_value(&request)?;

        // Send the json request
        self.conn.send(req_json).await?;
        debug!("--> {request}");

        // Register a new request
        let rx = self.message_dispatcher.register(id).await;

        // Wait for the message dispatcher to send the response
        let result = match self.timeout {
            Some(t) => timeout(Duration::from_millis(t), rx.recv()).await?,
            None => rx.recv().await,
        };

        let response = match result {
            Ok(r) => r,
            Err(err) => {
                // Unregister the request if an error occurs
                self.message_dispatcher.unregister(&id).await;
                return Err(err.into());
            }
        };

        if let Some(error) = response.error {
            return Err(Error::SubscribeError(error.code, error.message));
        }

        // It should be OK to unwrap here, as the message dispatcher checks
        // for the response id.
        if *response.id.as_ref().unwrap() != request.id {
            return Err(Error::InvalidMsg("Invalid response id"));
        }

        Ok(response)
    }

    fn start_background_receiving(self: &Arc<Self>) {
        let selfc = self.clone();
        let on_complete = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("Background receiving loop stopped: {err}");
            }
            // Drop all subscription
            selfc.subscriptions.drop_all().await;
        };

        let selfc = self.clone();
        // Spawn a new task for listing to new coming messages.
        self.task_group.spawn(
            async move {
                loop {
                    let msg = selfc.conn.recv().await?;
                    if let Err(err) = selfc.handle_msg(msg).await {
                        let endpoint = selfc.conn.peer_endpoint()?;
                        error!("Handle a new msg from the endpoint {endpoint} : {err}",);
                    }
                }
            },
            on_complete,
        );
    }

    async fn handle_msg(&self, msg: serde_json::Value) -> Result<()> {
        match serde_json::from_value::<NewMsg>(msg.clone()) {
            Ok(msg) => match msg {
                NewMsg::Response(res) => {
                    debug!("<-- {res}");
                    self.message_dispatcher.dispatch(res).await
                }
                NewMsg::Notification(nt) => {
                    debug!("<-- {nt}");
                    self.subscriptions.notify(nt).await?;
                    Ok(())
                }
            },
            Err(_) => {
                error!("Receive unexpected msg: {msg}");
                Err(Error::InvalidMsg("Unexpected msg"))
            }
        }
    }
}
