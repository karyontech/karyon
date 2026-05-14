//! Request sanity check and method dispatch.

use std::sync::Arc;

use log::debug;

use crate::{
    message,
    server::{
        channel::Channel, pubsub_service::PubSubRPCMethod, service::RPCMethod, Server,
        FAILED_TO_PARSE_ERROR_MSG, INVALID_REQUEST_ERROR_MSG, METHOD_NOT_FOUND_ERROR_MSG,
        UNSUPPORTED_JSONRPC_VERSION,
    },
};

pub(super) struct NewRequest {
    pub srvc_name: String,
    pub method_name: String,
    pub msg: message::Request,
}

pub(super) enum SanityCheckResult {
    NewReq(NewRequest),
    ErrRes(message::Response),
}

/// Resolved handler for a request. Holds whichever method matched.
pub(super) enum Handler<'a> {
    Pubsub(PubSubRPCMethod<'a>),
    Rpc(RPCMethod<'a>),
    None,
}

/// Parse and validate a raw JSON-RPC request.
pub(super) fn sanity_check(request: serde_json::Value) -> SanityCheckResult {
    let rpc_msg = match serde_json::from_value::<message::Request>(request) {
        Ok(m) => m,
        Err(_) => return err_res(None, message::PARSE_ERROR_CODE, FAILED_TO_PARSE_ERROR_MSG),
    };

    if rpc_msg.jsonrpc != message::JSONRPC_VERSION {
        return err_res(
            Some(rpc_msg.id),
            message::INVALID_REQUEST_ERROR_CODE,
            UNSUPPORTED_JSONRPC_VERSION,
        );
    }

    debug!("<-- {rpc_msg}");

    let srvc_method: Vec<&str> = rpc_msg.method.split('.').collect();
    if srvc_method.len() < 2 {
        return err_res(
            Some(rpc_msg.id),
            message::INVALID_REQUEST_ERROR_CODE,
            INVALID_REQUEST_ERROR_MSG,
        );
    }

    let srvc_name = srvc_method[0].to_string();
    let method_name = srvc_method[1..].join(".");
    SanityCheckResult::NewReq(NewRequest {
        srvc_name,
        method_name,
        msg: rpc_msg,
    })
}

fn err_res(id: Option<serde_json::Value>, code: i32, message: &str) -> SanityCheckResult {
    SanityCheckResult::ErrRes(message::Response {
        error: Some(message::Error {
            code,
            message: message.to_string(),
            data: None,
        }),
        id,
        ..Default::default()
    })
}

fn method_not_found(id: Option<serde_json::Value>) -> message::Response {
    message::Response {
        error: Some(message::Error {
            code: message::METHOD_NOT_FOUND_ERROR_CODE,
            message: METHOD_NOT_FOUND_ERROR_MSG.to_string(),
            data: None,
        }),
        id,
        ..Default::default()
    }
}

impl Server {
    /// Resolve a request to its handler. Pubsub handlers take priority.
    /// If `allow_pubsub` is false, only regular RPC services are checked.
    pub(super) fn resolve_handler(
        &self,
        srvc_name: &str,
        method_name: &str,
        allow_pubsub: bool,
    ) -> Handler<'_> {
        if allow_pubsub {
            if let Some(method) = self
                .config
                .pubsub_services
                .get(srvc_name)
                .and_then(|s| s.get_pubsub_method(method_name))
            {
                return Handler::Pubsub(method);
            }
        }
        if let Some(method) = self
            .config
            .services
            .get(srvc_name)
            .and_then(|s| s.get_method(method_name))
        {
            return Handler::Rpc(method);
        }
        Handler::None
    }

    /// Dispatch a JSON request. Pass `None` for `channel` when called
    /// from a context without pubsub support (HTTP/1.x and HTTP/2).
    pub(crate) async fn handle_request(
        &self,
        channel: Option<Arc<Channel>>,
        msg: serde_json::Value,
    ) -> message::Response {
        let req = match sanity_check(msg) {
            SanityCheckResult::NewReq(req) => req,
            SanityCheckResult::ErrRes(res) => return res,
        };

        let id = Some(req.msg.id.clone());
        let params = req.msg.params.unwrap_or(serde_json::json!(()));

        match self.resolve_handler(&req.srvc_name, &req.method_name, channel.is_some()) {
            Handler::Pubsub(method) => {
                let chan = channel.expect("channel required for pubsub");
                match method(chan, req.msg.method, params).await {
                    Ok(res) => message::Response {
                        result: Some(res),
                        id,
                        ..Default::default()
                    },
                    Err(err) => err.to_response(id, None),
                }
            }
            Handler::Rpc(method) => match method(params).await {
                Ok(res) => message::Response {
                    result: Some(res),
                    id,
                    ..Default::default()
                },
                Err(err) => err.to_response(id, None),
            },
            Handler::None => method_not_found(id),
        }
    }
}
