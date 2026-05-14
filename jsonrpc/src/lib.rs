#![doc = include_str!("../README.md")]

pub mod client;
pub mod codec;
pub mod error;
pub mod message;
pub mod server;

#[cfg(feature = "http")]
mod hyper_exec;

pub use karyon_jsonrpc_macro::{rpc_impl, rpc_method, rpc_pubsub_impl};
