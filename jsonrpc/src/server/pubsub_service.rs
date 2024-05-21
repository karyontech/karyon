use std::{future::Future, pin::Pin};

use crate::Result;

use super::channel::ArcChannel;

/// Represents the RPC method
pub type PubSubRPCMethod<'a> =
    Box<dyn Fn(ArcChannel, serde_json::Value) -> PubSubRPCMethodOutput<'a> + Send + 'a>;
type PubSubRPCMethodOutput<'a> =
    Pin<Box<dyn Future<Output = Result<serde_json::Value>> + Send + Sync + 'a>>;

/// Defines the interface for an RPC service.
pub trait PubSubRPCService: Sync + Send {
    fn get_pubsub_method<'a>(&'a self, name: &'a str) -> Option<PubSubRPCMethod>;
    fn name(&self) -> String;
}

/// Implements the [`PubSubRPCService`] trait for a provided type.
///
/// # Example
///
/// ```
/// use serde_json::Value;
///
/// use karyon_jsonrpc::{Error, impl_rpc_service};
///
/// struct Hello {}
///
/// impl Hello {
///     async fn foo(&self, params: Value) -> Result<Value, Error> {
///         Ok(serde_json::json!("foo!"))
///     }
///
///     async fn bar(&self, params: Value) -> Result<Value, Error> {
///         Ok(serde_json::json!("bar!"))
///     }
/// }
///
/// impl_rpc_service!(Hello, foo, bar);
///
/// ```
#[macro_export]
macro_rules! impl_pubsub_rpc_service {
    ($t:ty, $($m:ident),*) => {
        impl karyon_jsonrpc::PubSubRPCService for $t {
            fn get_pubsub_method<'a>(
                &'a self,
                name: &'a str
            ) -> Option<karyon_jsonrpc::PubSubRPCMethod> {
                match name {
                $(
                    stringify!($m) => {
                        Some(Box::new(move |chan: karyon_jsonrpc::ArcChannel, params: serde_json::Value| Box::pin(self.$m(chan, params))))
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
