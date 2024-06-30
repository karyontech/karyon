use std::{future::Future, pin::Pin};

use crate::RPCResult;

/// Represents the RPC method
pub type RPCMethod<'a> = Box<dyn Fn(serde_json::Value) -> RPCMethodOutput<'a> + Send + 'a>;
type RPCMethodOutput<'a> =
    Pin<Box<dyn Future<Output = RPCResult<serde_json::Value>> + Send + Sync + 'a>>;

/// Defines the interface for an RPC service.
pub trait RPCService: Sync + Send {
    fn get_method<'a>(&'a self, name: &'a str) -> Option<RPCMethod>;
    fn name(&self) -> String;
}
