pub mod builder;
pub mod channel;
pub mod pubsub_service;
pub mod service;

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use log::{debug, error, trace, warn};

use karyon_core::{
    async_runtime::lock::Mutex,
    async_util::{select, CondVar, Either, TaskGroup, TaskResult},
};
use karyon_net::{Conn, Endpoint, Listener};

use crate::{message, Error, PubSubRPCService, RPCService, Result};

use channel::{ArcChannel, Channel};

const CHANNEL_CAP: usize = 10;

pub const INVALID_REQUEST_ERROR_MSG: &str = "Invalid request";
pub const FAILED_TO_PARSE_ERROR_MSG: &str = "Failed to parse";
pub const METHOD_NOT_FOUND_ERROR_MSG: &str = "Method not found";
pub const INTERNAL_ERROR_MSG: &str = "Internal error";

fn pack_err_res(code: i32, msg: &str, id: Option<serde_json::Value>) -> message::Response {
    let err = message::Error {
        code,
        message: msg.to_string(),
        data: None,
    };

    message::Response {
        jsonrpc: message::JSONRPC_VERSION.to_string(),
        error: Some(err),
        result: None,
        id,
    }
}

struct NewRequest {
    srvc_name: String,
    method_name: String,
    msg: message::Request,
}

enum SanityCheckResult {
    NewReq(NewRequest),
    ErrRes(message::Response),
}

struct ResponseQueue<T> {
    queue: Mutex<VecDeque<T>>,
    condvar: CondVar,
}

impl<T> ResponseQueue<T> {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(VecDeque::new()),
            condvar: CondVar::new(),
        })
    }

    /// Wait while the queue is empty, remove and return the item from the queue,
    /// panicking if empty (shouldn't happen)
    async fn recv(&self) -> T {
        let mut queue = self.queue.lock().await;

        while queue.is_empty() {
            queue = self.condvar.wait(queue).await;
        }

        match queue.pop_front() {
            Some(v) => v,
            None => unreachable!(),
        }
    }

    /// Push an item into the queue, notify all waiting tasks that the
    /// condvar has changed
    async fn push(&self, res: T) {
        self.queue.lock().await.push_back(res);
        self.condvar.broadcast();
    }
}

/// Represents an RPC server
pub struct Server {
    listener: Listener<serde_json::Value>,
    task_group: TaskGroup,
    services: HashMap<String, Arc<dyn RPCService + 'static>>,
    pubsub_services: HashMap<String, Arc<dyn PubSubRPCService + 'static>>,
}

impl Server {
    /// Returns the local endpoint.
    pub fn local_endpoint(&self) -> Result<Endpoint> {
        self.listener.local_endpoint().map_err(Error::from)
    }

    /// Starts the RPC server
    pub async fn start(self: &Arc<Self>) {
        let on_failure = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("Accept loop stopped: {err}");
            }
        };

        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                loop {
                    match selfc.listener.accept().await {
                        Ok(conn) => {
                            if let Err(err) = selfc.handle_conn(conn).await {
                                error!("Failed to handle a new conn: {err}")
                            }
                        }
                        Err(err) => {
                            error!("Failed to accept a new conn: {err}")
                        }
                    }
                }
            },
            on_failure,
        );
    }

    /// Shuts down the RPC server
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Handles a new connection
    async fn handle_conn(self: &Arc<Self>, conn: Conn<serde_json::Value>) -> Result<()> {
        let endpoint = conn.peer_endpoint().expect("get peer endpoint");
        debug!("Handle a new connection {endpoint}");

        let (ch_tx, ch_rx) = async_channel::bounded(CHANNEL_CAP);
        let channel = Channel::new(ch_tx);

        let queue = ResponseQueue::new();

        let chan = channel.clone();
        let on_failure = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                debug!("Notification loop stopped: {err}");
            }
            // close the subscription channel
            chan.close();
        };

        let queue_cloned = queue.clone();
        self.task_group.spawn(
            async move {
                loop {
                    let nt = ch_rx.recv().await?;
                    let params = Some(serde_json::json!(message::NotificationResult {
                        subscription: nt.sub_id,
                        result: Some(nt.result),
                    }));
                    let response = message::Notification {
                        jsonrpc: message::JSONRPC_VERSION.to_string(),
                        method: nt.method,
                        params,
                    };
                    debug!("--> {response}");
                    queue_cloned.push(serde_json::json!(response)).await;
                }
            },
            on_failure,
        );

        let chan = channel.clone();
        let on_failure = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("Connection {} dropped: {}", endpoint, err);
            } else {
                warn!("Connection {} dropped", endpoint);
            }
            // close the subscription channel when the connection dropped
            chan.close();
        };

        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                loop {
                    match select(conn.recv(), queue.recv()).await {
                        Either::Left(msg) => {
                            selfc
                                .new_request(queue.clone(), channel.clone(), msg?)
                                .await;
                        }
                        Either::Right(res) => conn.send(res).await?,
                    }
                }
            },
            on_failure,
        );

        Ok(())
    }

    fn sanity_check(&self, request: serde_json::Value) -> SanityCheckResult {
        let rpc_msg = match serde_json::from_value::<message::Request>(request) {
            Ok(m) => m,
            Err(_) => {
                return SanityCheckResult::ErrRes(pack_err_res(
                    message::PARSE_ERROR_CODE,
                    FAILED_TO_PARSE_ERROR_MSG,
                    None,
                ));
            }
        };
        debug!("<-- {rpc_msg}");

        let srvc_method_str = rpc_msg.method.clone();
        let srvc_method: Vec<&str> = srvc_method_str.split('.').collect();
        if srvc_method.len() < 2 {
            return SanityCheckResult::ErrRes(pack_err_res(
                message::INVALID_REQUEST_ERROR_CODE,
                INVALID_REQUEST_ERROR_MSG,
                Some(rpc_msg.id),
            ));
        }

        let srvc_name = srvc_method[0].to_string();
        let method_name = srvc_method[1].to_string();

        SanityCheckResult::NewReq(NewRequest {
            srvc_name,
            method_name,
            msg: rpc_msg,
        })
    }

    /// Spawns a new task for handling a new request
    async fn new_request(
        self: &Arc<Self>,
        queue: Arc<ResponseQueue<serde_json::Value>>,
        channel: ArcChannel,
        msg: serde_json::Value,
    ) {
        trace!("--> new request {msg}");
        let on_failure = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("Failed to handle a request: {err}");
            }
        };
        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                let response = selfc.handle_request(channel, msg).await;
                debug!("--> {response}");
                queue.push(serde_json::json!(response)).await;
                Ok(())
            },
            on_failure,
        );
    }

    /// Handles a new request
    async fn handle_request(
        &self,
        channel: ArcChannel,
        msg: serde_json::Value,
    ) -> message::Response {
        let req = match self.sanity_check(msg) {
            SanityCheckResult::NewReq(req) => req,
            SanityCheckResult::ErrRes(res) => return res,
        };

        let mut response = message::Response {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            error: None,
            result: None,
            id: Some(req.msg.id.clone()),
        };

        if let Some(service) = self.pubsub_services.get(&req.srvc_name) {
            if let Some(method) = service.get_pubsub_method(&req.method_name) {
                let name = format!("{}.{}", service.name(), req.method_name);
                let params = req.msg.params.unwrap_or(serde_json::json!(()));
                response.result = match method(channel, name, params).await {
                    Ok(res) => Some(res),
                    Err(err) => return self.handle_error(err, req.msg.id),
                };

                return response;
            }
        }

        if let Some(service) = self.services.get(&req.srvc_name) {
            if let Some(method) = service.get_method(&req.method_name) {
                let params = req.msg.params.unwrap_or(serde_json::json!(()));
                response.result = match method(params).await {
                    Ok(res) => Some(res),
                    Err(err) => return self.handle_error(err, req.msg.id),
                };

                return response;
            }
        }

        pack_err_res(
            message::METHOD_NOT_FOUND_ERROR_CODE,
            METHOD_NOT_FOUND_ERROR_MSG,
            Some(req.msg.id),
        )
    }

    fn handle_error(&self, err: Error, msg_id: serde_json::Value) -> message::Response {
        match err {
            Error::ParseJSON(_) => pack_err_res(
                message::PARSE_ERROR_CODE,
                FAILED_TO_PARSE_ERROR_MSG,
                Some(msg_id),
            ),
            Error::InvalidParams(msg) => {
                pack_err_res(message::INVALID_PARAMS_ERROR_CODE, msg, Some(msg_id))
            }
            Error::InvalidRequest(msg) => {
                pack_err_res(message::INVALID_REQUEST_ERROR_CODE, msg, Some(msg_id))
            }
            Error::RPCMethodError(code, msg) => pack_err_res(code, msg, Some(msg_id)),
            _ => pack_err_res(
                message::INTERNAL_ERROR_CODE,
                INTERNAL_ERROR_MSG,
                Some(msg_id),
            ),
        }
    }
}
