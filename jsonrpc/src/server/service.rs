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

/// Implements the [`RPCService`] trait for a provided type.
///
/// # Example
///
/// ```
/// use serde_json::Value;
///
/// use karyon_jsonrpc::{RPCError, impl_rpc_service};
///
/// struct Hello {}
///
/// impl Hello {
///     async fn foo(&self, params: Value) -> Result<Value, RPCError> {
///         Ok(serde_json::json!("foo!"))
///     }
///
///     async fn bar(&self, params: Value) -> Result<Value, RPCError> {
///         Ok(serde_json::json!("bar!"))
///     }
/// }
///
/// impl_rpc_service!(Hello, foo, bar);
///
/// ```
#[macro_export]
macro_rules! impl_rpc_service {
    ($t:ty, $($m:ident),*) => {
        impl karyon_jsonrpc::RPCService for $t {
            fn get_method<'a>(
                &'a self,
                name: &'a str
            ) -> Option<karyon_jsonrpc::RPCMethod> {
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
