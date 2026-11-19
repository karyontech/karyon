use std::{future::Future, pin::Pin, sync::Arc};

use serde_json::Value;

use crate::error::RPCResult;

use super::channel::ChannelRef;

/// Represents the RPC method
pub type PubSubRPCMethod<'a> =
    Box<dyn Fn(ChannelRef, String, Value) -> PubSubRPCMethodOutput<'a> + Send + 'a>;
type PubSubRPCMethodOutput<'a> = Pin<Box<dyn Future<Output = RPCResult<Value>> + Send + Sync + 'a>>;

/// Defines the interface for an RPC service.
pub trait PubSubRPCService: Sync + Send {
    fn get_pubsub_method(&self, name: &str) -> Option<PubSubRPCMethod<'_>>;
    fn name(&self) -> String;
}

/// `Arc` reference to a `PubSubRPCService`.
pub type PubSubRPCServiceRef = Arc<dyn PubSubRPCService>;
