use serde::{Deserialize, Serialize};

use crate::SubscriptionID;

pub type ID = u64;

pub const JSONRPC_VERSION: &str = "2.0";

/// Parse error: Invalid JSON was received by the server.
pub const PARSE_ERROR_CODE: i32 = -32700;

/// Invalid request: The JSON sent is not a valid Request object.
pub const INVALID_REQUEST_ERROR_CODE: i32 = -32600;

/// Method not found: The method does not exist / is not available.
pub const METHOD_NOT_FOUND_ERROR_CODE: i32 = -32601;

/// Invalid params: Invalid method parameter(s).
pub const INVALID_PARAMS_ERROR_CODE: i32 = -32602;

/// Internal error: Internal JSON-RPC error.
pub const INTERNAL_ERROR_CODE: i32 = -32603;

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscriber: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationResult {
    pub result: Option<serde_json::Value>,
    pub subscription: SubscriptionID,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Error {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl std::fmt::Display for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{jsonrpc: {}, method: {}, params: {:?}, id: {:?}, subscribe: {:?}}}",
            self.jsonrpc, self.method, self.params, self.id, self.subscriber
        )
    }
}

impl std::fmt::Display for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{jsonrpc: {}, result': {:?}, error: {:?} , id: {:?}}}",
            self.jsonrpc, self.result, self.error, self.id,
        )
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RpcError {{ code: {}, message: {}, data: {:?} }} ",
            self.code, self.message, self.data
        )
    }
}

impl std::fmt::Display for Notification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{jsonrpc: {}, method: {:?}, params: {:?}}}",
            self.jsonrpc, self.method, self.params
        )
    }
}
