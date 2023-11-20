use std::{future::Future, pin::Pin};

use crate::Result;

/// Represents the RPC method
pub type RPCMethod<'a> = Box<dyn Fn(serde_json::Value) -> RPCMethodOutput<'a> + Send + 'a>;
type RPCMethodOutput<'a> =
    Pin<Box<dyn Future<Output = Result<serde_json::Value>> + Send + Sync + 'a>>;

/// Defines the interface for an RPC service.
pub trait RPCService: Sync + Send {
    fn get_method<'a>(&'a self, name: &'a str) -> Option<RPCMethod>;
    fn name(&self) -> String;
}

/// Implements the `RPCService` trait for a provided type.
///
/// # Example
///
/// ```
/// use serde_json::Value;
///
/// use karyons_jsonrpc::{JsonRPCError, register_service};
///
/// struct Hello {}
///
/// impl Hello {
///     async fn say_hello(&self, params: Value) -> Result<Value, JsonRPCError> {
///         Ok(serde_json::json!("hello!"))
///     }
/// }
///
/// register_service!(Hello, say_hello);
///
/// ```
#[macro_export]
macro_rules! register_service {
    ($t:ty, $($m:ident),*) => {
        impl karyons_jsonrpc::RPCService for $t {
            fn get_method<'a>(
                &'a self,
                name: &'a str
            ) -> Option<karyons_jsonrpc::RPCMethod> {
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
