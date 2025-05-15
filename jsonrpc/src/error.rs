use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

/// Represents karyon's jsonrpc Error.
#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Call Error: code: {0} - msg: {1}")]
    CallError(i32, String),

    #[error("Subscribe Error: code: {0} - msg: {1}")]
    SubscribeError(i32, String),

    #[error("Encode Error: {0}")]
    Encode(String),

    #[error("Decode Error: {0}")]
    Decode(String),

    #[error("Invalid Message Error: {0}")]
    InvalidMsg(String),

    #[error(transparent)]
    ParseJSON(#[from] serde_json::Error),

    #[error("Unsupported Protocol: {0}")]
    UnsupportedProtocol(String),

    #[error("Tls config is required")]
    TLSConfigRequired,

    #[error("Receive Close Message From Connection: {0}")]
    CloseConnection(String),

    #[error("Subscription Not Found: {0}")]
    SubscriptionNotFound(String),

    #[error("Subscription Exceeds The Maximum Buffer Size")]
    SubscriptionBufferFull,

    #[error("Subscription Closed")]
    SubscriptionClosed,

    #[error("Subscription duplicated: {0}")]
    SubscriptionDuplicated(String),

    #[error("ClientDisconnected")]
    ClientDisconnected,

    #[error(transparent)]
    ChannelRecv(#[from] async_channel::RecvError),

    #[error("Channel send  Error: {0}")]
    ChannelSend(String),

    #[cfg(feature = "ws")]
    #[error(transparent)]
    WebSocket(#[from] async_tungstenite::tungstenite::Error),

    #[error("Unexpected Error: {0}")]
    General(String),

    #[error(transparent)]
    KaryonCore(#[from] karyon_core::error::Error),

    #[error(transparent)]
    KaryonNet(#[from] karyon_net::Error),
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(error: async_channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}

pub type RPCResult<T> = std::result::Result<T, RPCError>;

/// Represents RPC Error.
#[derive(ThisError, Debug)]
pub enum RPCError {
    #[error("Custom Error:  code: {0} msg: {1}")]
    CustomError(i32, String),

    #[error("Invalid Params: {0}")]
    InvalidParams(String),

    #[error("Invalid Request: {0}")]
    InvalidRequest(String),

    #[error("Parse Error: {0}")]
    ParseError(String),

    #[error("Internal Error")]
    InternalError,
}

impl From<serde_json::Error> for RPCError {
    fn from(error: serde_json::Error) -> Self {
        RPCError::ParseError(error.to_string())
    }
}
