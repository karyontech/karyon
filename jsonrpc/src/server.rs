use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use log::{debug, error, warn};
use smol::lock::RwLock;

use karyons_core::{
    async_utils::{TaskGroup, TaskResult},
    Executor,
};
use karyons_net::{listen, Conn, Endpoint, Listener};

use crate::{
    message,
    utils::{read_until, write_all},
    Error, Result, JSONRPC_VERSION,
};

/// Represents an RPC server
pub struct Server<'a> {
    listener: Box<dyn Listener>,
    services: RwLock<HashMap<String, Box<dyn RPCService + 'a>>>,
    task_group: TaskGroup<'a>,
}

impl<'a> Server<'a> {
    /// Creates a new RPC server.
    pub fn new(listener: Box<dyn Listener>, ex: Executor<'a>) -> Arc<Self> {
        Arc::new(Self {
            listener,
            services: RwLock::new(HashMap::new()),
            task_group: TaskGroup::new(ex),
        })
    }

    /// Creates a new RPC server using the provided endpoint.
    pub async fn new_with_endpoint(endpoint: &Endpoint, ex: Executor<'a>) -> Result<Arc<Self>> {
        let listener = listen(endpoint).await?;
        Ok(Arc::new(Self {
            listener,
            services: RwLock::new(HashMap::new()),
            task_group: TaskGroup::new(ex),
        }))
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

        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                loop {
                    let mut buffer = vec![];
                    read_until(&conn, &mut buffer).await?;
                    let response = selfc.handle_request(&buffer).await;
                    let payload = serde_json::to_vec(&response)?;
                    write_all(&conn, &payload).await?;
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

/// Represents the RPC method
pub type RPCMethod<'a> = Box<dyn Fn(serde_json::Value) -> RPCMethodOutput<'a> + Send + 'a>;
type RPCMethodOutput<'a> =
    Pin<Box<dyn Future<Output = Result<serde_json::Value>> + Send + Sync + 'a>>;

/// Defines the interface for an RPC service.
pub trait RPCService: Sync + Send {
    fn get_method<'a>(&'a self, name: &'a str) -> Option<RPCMethod>;
    fn name(&self) -> String;
}

/// Implements the `RPCService` trait for a provided type.
///
/// # Example
///
/// ```
/// use serde_json::Value;
///
/// use karyons_jsonrpc::{JsonRPCError, register_service};
///
/// struct Hello {}
///
/// impl Hello {
///     async fn say_hello(&self, params: Value) -> Result<Value, JsonRPCError> {
///         Ok(serde_json::json!("hello!"))
///     }
/// }
///
/// register_service!(Hello, say_hello);
///
/// ```
#[macro_export]
macro_rules! register_service {
    ($t:ty, $($m:ident),*) => {
        impl karyons_jsonrpc::RPCService for $t {
            fn get_method<'a>(
                &'a self,
                name: &'a str
            ) -> Option<karyons_jsonrpc::RPCMethod> {
                match name {
                $(
                    stringify!($m) => {
                        Some(Box::new(move |params: serde_json::Value| Box::pin(self.$m(params))))
                    }
                )*
                    _ => None,
                }


            }
            fn name(&self) -> String{
                stringify!($t).to_string()
            }
        }
    };
}
