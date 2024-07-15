use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("TryInto Error: {0}")]
    TryInto(&'static str),

    #[error("Timeout Error")]
    Timeout,

    #[error("Path Not Found Error: {0}")]
    PathNotFound(&'static str),

    #[error("Event Emit Error: {0}")]
    EventEmitError(String),

    #[cfg(feature = "crypto")]
    #[error(transparent)]
    Ed25519(#[from] ed25519_dalek::ed25519::Error),

    #[cfg(feature = "tokio")]
    #[error(transparent)]
    TokioJoinError(#[from] tokio::task::JoinError),

    #[error("Channel Send Error: {0}")]
    ChannelSend(String),

    #[error(transparent)]
    ChannelRecv(#[from] async_channel::RecvError),

    #[error(transparent)]
    BincodeDecode(#[from] bincode::error::DecodeError),

    #[error(transparent)]
    BincodeEncode(#[from] bincode::error::EncodeError),
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(error: async_channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}
