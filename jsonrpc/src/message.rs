use serde::{Deserialize, Serialize};

use crate::RPCError;

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

/// SubscriptionID is used to identify a subscription.
pub type SubscriptionID = u32;

pub const INTERNAL_ERROR_MSG: &str = "Internal error";

/// Request represents a JSON-RPC request message.
/// It includes the JSON-RPC version, an identifier for the request, the method
/// to be invoked, and optional parameters.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    /// JSON-RPC version, typically "2.0".
    pub jsonrpc: String,
    /// Unique identifier for the request, can be a number or a string.
    pub id: serde_json::Value,
    /// The name of the method to be invoked.
    pub method: String,
    /// Optional parameters for the method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// Response represents a JSON-RPC response message.
/// It includes the JSON-RPC version, an identifier matching the request, the result of the request,
/// and an optional error.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    /// JSON-RPC version, typically "2.0".
    pub jsonrpc: String,
    /// Unique identifier for the request, can be a number or a string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    /// Result of the request if it was successful.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error object if the request failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

/// Notification represents a JSON-RPC notification message.
#[derive(Debug, Serialize, Deserialize)]
pub struct Notification {
    /// JSON-RPC version, typically "2.0".
    pub jsonrpc: String,
    /// The name of the method to be invoked.
    pub method: String,
    /// Optional parameters for the method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// NotificationResult represents the result of a subscription notification.
/// It includes the result and the subscription ID that triggered the notification.
#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationResult {
    /// Optional data about the notification.
    pub result: Option<serde_json::Value>,
    /// ID of the subscription that triggered the notification.
    pub subscription: SubscriptionID,
}

// Error represents an error in a JSON-RPC response.
// It includes an error code, a message, and optional additional data.
#[derive(Debug, Serialize, Deserialize)]
pub struct Error {
    /// Error code indicating the type of error.
    pub code: i32,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional data about the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl std::fmt::Display for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{jsonrpc: {}, method: {}, params: {:?}, id: {:?}}}",
            self.jsonrpc, self.method, self.params, self.id,
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

impl Default for Response {
    fn default() -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            error: None,
            id: None,
            result: None,
        }
    }
}

impl RPCError {
    pub fn to_response(
        &self,
        id: Option<serde_json::Value>,
        data: Option<serde_json::Value>,
    ) -> Response {
        let err: Error = match self {
            RPCError::ParseError(msg) => Error {
                code: PARSE_ERROR_CODE,
                message: msg.to_string(),
                data,
            },
            RPCError::InvalidParams(msg) => Error {
                code: INVALID_PARAMS_ERROR_CODE,
                message: msg.to_string(),
                data,
            },
            RPCError::InvalidRequest(msg) => Error {
                code: INVALID_REQUEST_ERROR_CODE,
                message: msg.to_string(),
                data,
            },
            RPCError::CustomError(code, msg) => Error {
                code: *code,
                message: msg.to_string(),
                data,
            },
            RPCError::InternalError => Error {
                code: INTERNAL_ERROR_CODE,
                message: INTERNAL_ERROR_MSG.to_string(),
                data,
            },
        };

        Response {
            jsonrpc: JSONRPC_VERSION.to_string(),
            error: Some(err),
            result: None,
            id,
        }
    }
}
