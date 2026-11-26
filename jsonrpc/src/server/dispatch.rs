//! Request sanity check and method dispatch.

use log::debug;

use crate::{
    message,
    server::{
        FAILED_TO_PARSE_ERROR_MSG, INVALID_REQUEST_ERROR_MSG, METHOD_NOT_FOUND_ERROR_MSG, Server,
        UNSUPPORTED_JSONRPC_VERSION, channel::ChannelRef, pubsub_service::PubSubRPCMethod,
        service::RPCMethod,
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
pub(super) fn sanity_check(server: &Server, request: serde_json::Value) -> SanityCheckResult {
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

    let (srvc_name, method_name) = match rpc_msg.method.split_once('.') {
        Some(names) => names,
        None if server.config.methods.contains_key(&rpc_msg.method) => {
            ("", rpc_msg.method.as_str())
        }
        None => {
            return err_res(
                Some(rpc_msg.id),
                message::INVALID_REQUEST_ERROR_CODE,
                INVALID_REQUEST_ERROR_MSG,
            );
        }
    };

    SanityCheckResult::NewReq(NewRequest {
        srvc_name: srvc_name.to_owned(),
        method_name: method_name.to_owned(),
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
    pub(super) fn resolve_handler(&self, req: &NewRequest, allow_pubsub: bool) -> Handler<'_> {
        if allow_pubsub
            && let Some(method) = self
                .config
                .pubsub_services
                .get(&req.srvc_name)
                .and_then(|s| s.get_pubsub_method(&req.method_name))
        {
            return Handler::Pubsub(method);
        }
        if let Some(method) = self.config.methods.get(&req.msg.method) {
            return Handler::Rpc(Box::new(move |params| method(params)));
        }
        if let Some(method) = self.config.service_method(&req.srvc_name, &req.method_name) {
            return Handler::Rpc(method);
        }
        Handler::None
    }

    /// Dispatch a JSON request. Pass `None` for `channel` when called
    /// from a context without pubsub support (HTTP/1.x and HTTP/2).
    pub(crate) async fn handle_request(
        &self,
        channel: Option<ChannelRef>,
        msg: serde_json::Value,
    ) -> message::Response {
        let req = match sanity_check(self, msg) {
            SanityCheckResult::NewReq(req) => req,
            SanityCheckResult::ErrRes(res) => return res,
        };
        let handler = self.resolve_handler(&req, channel.is_some());
        run_handler(handler, req.msg, channel).await
    }
}

/// Run a resolved handler. A pubsub handler needs a `channel`.
pub(super) async fn run_handler(
    handler: Handler<'_>,
    req: message::Request,
    channel: Option<ChannelRef>,
) -> message::Response {
    let id = Some(req.id);
    let params = req.params.unwrap_or(serde_json::json!(()));

    let result = match (handler, channel) {
        (Handler::Pubsub(method), Some(chan)) => method(chan, req.method, params).await,
        (Handler::Rpc(method), _) => method(params).await,
        _ => return method_not_found(id),
    };

    match result {
        Ok(res) => message::Response {
            result: Some(res),
            id,
            ..Default::default()
        },
        Err(err) => err.to_response(id, None),
    }
}
