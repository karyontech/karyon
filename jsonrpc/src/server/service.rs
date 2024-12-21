use std::{future::Future, pin::Pin};

use crate::error::RPCResult;

/// Represents the RPC method
pub type RPCMethod<'a> = Box<dyn Fn(serde_json::Value) -> RPCMethodOutput<'a> + Send + 'a>;
type RPCMethodOutput<'a> =
    Pin<Box<dyn Future<Output = RPCResult<serde_json::Value>> + Send + Sync + 'a>>;

/// Defines the interface for an RPC service.
pub trait RPCService: Sync + Send {
    fn get_method(&self, name: &str) -> Option<RPCMethod>;
    fn name(&self) -> String;
}
