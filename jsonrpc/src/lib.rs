#![doc = include_str!("../README.md")]

mod client;
mod codec;
mod error;
pub mod message;
mod server;

pub use client::Client;
pub use error::{Error, Result};
pub use server::{
    channel::{ArcChannel, Channel, Subscription, SubscriptionID},
    pubsub_service::{PubSubRPCMethod, PubSubRPCService},
    service::{RPCMethod, RPCService},
    Server,
};

pub use karyon_jsonrpc_macro::{rpc_impl, rpc_pubsub_impl};

pub use karyon_net::{tcp::TcpConfig, Endpoint};
