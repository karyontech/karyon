#![doc = include_str!("../README.md")]

#[cfg(all(
    target_family = "wasm",
    any(
        feature = "tcp",
        feature = "unix",
        feature = "ws",
        feature = "quic",
        feature = "http"
    )
))]
compile_error!(
    "wasm has no built-in sockets: build with `--no-default-features --features tokio` \
     and bring them with `karyon_net::transport::Transport`."
);

pub mod client;
pub mod codec;
pub mod error;
pub mod message;
pub mod server;

#[cfg(feature = "http")]
mod hyper_exec;

pub use karyon_jsonrpc_macro::{rpc_impl, rpc_method, rpc_pubsub_impl};
