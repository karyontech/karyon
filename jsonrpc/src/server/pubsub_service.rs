use std::{future::Future, pin::Pin, sync::Arc};

use crate::error::RPCResult;

use super::channel::Channel;

/// Represents the RPC method
pub type PubSubRPCMethod<'a> =
    Box<dyn Fn(Arc<Channel>, String, serde_json::Value) -> PubSubRPCMethodOutput<'a> + Send + 'a>;
type PubSubRPCMethodOutput<'a> =
    Pin<Box<dyn Future<Output = RPCResult<serde_json::Value>> + Send + Sync + 'a>>;

/// Defines the interface for an RPC service.
pub trait PubSubRPCService: Sync + Send {
    fn get_pubsub_method(&self, name: &str) -> Option<PubSubRPCMethod<'_>>;
    fn name(&self) -> String;
}
