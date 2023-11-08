use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("IO Error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Timeout Error")]
    Timeout,

    #[error("Path Not Found Error: {0}")]
    PathNotFound(&'static str),

    #[error("Channel Send Error: {0}")]
    ChannelSend(String),

    #[error("Channel Receive Error: {0}")]
    ChannelRecv(String),

    #[error("Decode Error: {0}")]
    Decode(String),

    #[error("Encode Error: {0}")]
    Encode(String),
}

impl<T> From<smol::channel::SendError<T>> for Error {
    fn from(error: smol::channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}

impl From<smol::channel::RecvError> for Error {
    fn from(error: smol::channel::RecvError) -> Self {
        Error::ChannelRecv(error.to_string())
    }
}

impl From<bincode::error::DecodeError> for Error {
    fn from(error: bincode::error::DecodeError) -> Self {
        Error::Decode(error.to_string())
    }
}

impl From<bincode::error::EncodeError> for Error {
    fn from(error: bincode::error::EncodeError) -> Self {
        Error::Encode(error.to_string())
    }
}
