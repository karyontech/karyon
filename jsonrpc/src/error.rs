use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

/// Represents karyon's jsonrpc Error.
#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Call Error: code: {0} msg: {1}")]
    CallError(i32, String),

    #[error("Subscribe Error: code: {0} msg: {1}")]
    SubscribeError(i32, String),

    #[error("Invalid Message Error: {0}")]
    InvalidMsg(&'static str),

    #[error(transparent)]
    ParseJSON(#[from] serde_json::Error),

    #[error("Unsupported protocol: {0}")]
    UnsupportedProtocol(String),

    #[error("Receive close message from connection: {0}")]
    CloseConnection(String),

    #[error("Subscription not found: {0}")]
    SubscriptionNotFound(String),

    #[error("Subscription exceeds the maximum buffer size")]
    SubscriptionBufferFull,

    #[error("ClientDisconnected")]
    ClientDisconnected,

    #[error(transparent)]
    ChannelRecv(#[from] async_channel::RecvError),

    #[error("Channel send  Error: {0}")]
    ChannelSend(&'static str),

    #[error("Unexpected Error: {0}")]
    General(&'static str),

    #[error(transparent)]
    KaryonCore(#[from] karyon_core::error::Error),

    #[error(transparent)]
    KaryonNet(#[from] karyon_net::Error),
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(error: async_channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string().leak())
    }
}

pub type RPCResult<T> = std::result::Result<T, RPCError>;

/// Represents RPC Error.
#[derive(ThisError, Debug)]
pub enum RPCError {
    #[error("Custom Error:  code: {0} msg: {1}")]
    CustomError(i32, &'static str),

    #[error("Invalid Params: {0}")]
    InvalidParams(&'static str),

    #[error("Invalid Request: {0}")]
    InvalidRequest(&'static str),

    #[error("Parse Error: {0}")]
    ParseError(&'static str),

    #[error("Internal Error")]
    InternalError,
}

impl From<serde_json::Error> for RPCError {
    fn from(error: serde_json::Error) -> Self {
        RPCError::ParseError(error.to_string().leak())
    }
}
