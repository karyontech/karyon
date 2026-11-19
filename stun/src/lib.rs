#![doc = include_str!("../README.md")]

#[cfg(all(feature = "smol", feature = "tokio"))]
compile_error!("Only one async runtime feature should be enabled");

#[cfg(not(any(feature = "smol", feature = "tokio")))]
compile_error!("At least one async runtime feature must be enabled for this crate.");

pub mod attrs;
pub mod client;
pub mod codec;
pub mod error;
pub mod msg;
pub mod server;

pub use attrs::Attribute;
pub use client::StunClient;
pub use codec::StunCodec;
pub use error::{Error, Result};
pub use msg::{MAGIC_COOKIE, MessageType, StunMessage, TransactionId};
pub use server::{StunConfig, StunServer, StunServerRef};
