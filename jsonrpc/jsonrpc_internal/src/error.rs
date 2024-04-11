use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

/// Represents karyon's jsonrpc Error.
#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Call Error: code: {0} msg: {1}")]
    CallError(i32, String),

    #[error("RPC Method Error:  code: {0} msg: {1}")]
    RPCMethodError(i32, &'static str),

    #[error("Invalid Params: {0}")]
    InvalidParams(&'static str),

    #[error("Invalid Request: {0}")]
    InvalidRequest(&'static str),

    #[error(transparent)]
    ParseJSON(#[from] serde_json::Error),

    #[error("Invalid Message Error: {0}")]
    InvalidMsg(&'static str),

    #[error("Unsupported protocol: {0}")]
    UnsupportedProtocol(String),

    #[error("Unexpected Error: {0}")]
    General(&'static str),

    #[error(transparent)]
    KaryonCore(#[from] karyon_core::error::Error),

    #[error(transparent)]
    KaryonNet(#[from] karyon_net::Error),
}
