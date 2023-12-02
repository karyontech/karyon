use std::{collections::HashMap, sync::Arc};

use log::{debug, error, warn};
use smol::lock::RwLock;

use karyon_core::{
    async_util::{TaskGroup, TaskResult},
    Executor,
};

use karyon_net::{Conn, Listener, ToListener};

use crate::{
    codec::{Codec, CodecConfig},
    message,
    service::RPCService,
    Endpoint, Error, Result, JSONRPC_VERSION,
};

/// RPC server config
#[derive(Default)]
pub struct ServerConfig {
    codec_config: CodecConfig,
}

/// Represents an RPC server
pub struct Server<'a> {
    listener: Listener,
    services: RwLock<HashMap<String, Box<dyn RPCService + 'a>>>,
    task_group: TaskGroup<'a>,
    config: ServerConfig,
}

impl<'a> Server<'a> {
    /// Creates a new RPC server by passing a listener. It supports Tcp, Unix, and Tls.
    pub fn new<T: ToListener>(listener: T, config: ServerConfig, ex: Executor<'a>) -> Arc<Self> {
        Arc::new(Self {
            listener: listener.to_listener(),
            services: RwLock::new(HashMap::new()),
            task_group: TaskGroup::new(ex),
            config,
        })
    }

    /// Returns the local endpoint.
    pub fn local_endpoint(&self) -> Result<Endpoint> {
        self.listener.local_endpoint().map_err(Error::KaryonNet)
    }

    /// Starts the RPC server
    pub async fn start(self: Arc<Self>) -> Result<()> {
        loop {
            let conn = self.listener.accept().await?;
            self.handle_conn(conn).await?;
        }
    }

    /// Attach a new service to the RPC server
    pub async fn attach_service(&self, service: impl RPCService + 'a) {
        self.services
            .write()
            .await
            .insert(service.name(), Box::new(service));
    }

    /// Shuts down the RPC server
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Handles a new connection
    async fn handle_conn(self: &Arc<Self>, conn: Conn) -> Result<()> {
        let endpoint = conn.peer_endpoint()?;
        debug!("Handle a new connection {endpoint}");

        let on_failure = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("Connection {} dropped: {}", endpoint, err);
            } else {
                warn!("Connection {} dropped", endpoint);
            }
        };

        let codec = Codec::new(conn, self.config.codec_config.clone());

        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                loop {
                    let mut buffer = vec![];
                    codec.read_until(&mut buffer).await?;
                    let response = selfc.handle_request(&buffer).await;
                    let mut payload = serde_json::to_vec(&response)?;
                    payload.push(b'\n');
                    codec.write_all(&payload).await?;
                    debug!("--> {response}");
                }
            },
            on_failure,
        );

        Ok(())
    }

    /// Handles a request
    async fn handle_request(&self, buffer: &[u8]) -> message::Response {
        let rpc_msg = match serde_json::from_slice::<message::Request>(buffer) {
            Ok(m) => m,
            Err(_) => {
                return self.pack_err_res(message::PARSE_ERROR_CODE, "Failed to parse", None);
            }
        };

        debug!("<-- {rpc_msg}");

        let srvc_method: Vec<&str> = rpc_msg.method.split('.').collect();
        if srvc_method.len() != 2 {
            return self.pack_err_res(
                message::INVALID_REQUEST_ERROR_CODE,
                "Invalid request",
                Some(rpc_msg.id),
            );
        }

        let srvc_name = srvc_method[0];
        let method_name = srvc_method[1];

        let services = self.services.read().await;

        let service = match services.get(srvc_name) {
            Some(s) => s,
            None => {
                return self.pack_err_res(
                    message::METHOD_NOT_FOUND_ERROR_CODE,
                    "Method not found",
                    Some(rpc_msg.id),
                );
            }
        };

        let method = match service.get_method(method_name) {
            Some(m) => m,
            None => {
                return self.pack_err_res(
                    message::METHOD_NOT_FOUND_ERROR_CODE,
                    "Method not found",
                    Some(rpc_msg.id),
                );
            }
        };

        let result = match method(rpc_msg.params.clone()).await {
            Ok(res) => res,
            Err(Error::ParseJSON(_)) => {
                return self.pack_err_res(
                    message::PARSE_ERROR_CODE,
                    "Failed to parse",
                    Some(rpc_msg.id),
                );
            }
            Err(Error::InvalidParams(msg)) => {
                return self.pack_err_res(
                    message::INVALID_PARAMS_ERROR_CODE,
                    msg,
                    Some(rpc_msg.id),
                );
            }
            Err(Error::InvalidRequest(msg)) => {
                return self.pack_err_res(
                    message::INVALID_REQUEST_ERROR_CODE,
                    msg,
                    Some(rpc_msg.id),
                );
            }
            Err(Error::RPCMethodError(code, msg)) => {
                return self.pack_err_res(code, msg, Some(rpc_msg.id));
            }
            Err(_) => {
                return self.pack_err_res(
                    message::INTERNAL_ERROR_CODE,
                    "Internal error",
                    Some(rpc_msg.id),
                );
            }
        };

        message::Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            error: None,
            result: Some(result),
            id: Some(rpc_msg.id),
        }
    }

    fn pack_err_res(
        &self,
        code: i32,
        msg: &str,
        id: Option<serde_json::Value>,
    ) -> message::Response {
        let err = message::Error {
            code,
            message: msg.to_string(),
            data: None,
        };

        message::Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            error: Some(err),
            result: None,
            id,
        }
    }
}
