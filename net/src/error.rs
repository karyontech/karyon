use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("IO Error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Try from endpoint Error")]
    TryFromEndpointError,

    #[error("invalid address {0}")]
    InvalidAddress(String),

    #[error("invalid endpoint {0}")]
    InvalidEndpoint(String),

    #[error("Parse endpoint error {0}")]
    ParseEndpoint(String),

    #[error("Timeout Error")]
    Timeout,

    #[error("Channel Send Error: {0}")]
    ChannelSend(String),

    #[error("Channel Receive Error: {0}")]
    ChannelRecv(String),

    #[error("Karyons core error : {0}")]
    KaryonsCore(#[from] karyons_core::error::Error),
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
