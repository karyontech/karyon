use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Try from endpoint Error")]
    TryFromEndpoint,

    #[error("invalid address {0}")]
    InvalidAddress(String),

    #[error("invalid path {0}")]
    InvalidPath(String),

    #[error("invalid endpoint {0}")]
    InvalidEndpoint(String),

    #[error("Encode error: {0}")]
    Encode(String),

    #[error("Decode error: {0}")]
    Decode(String),

    #[error("Parse endpoint error {0}")]
    ParseEndpoint(String),

    #[error("Timeout Error")]
    Timeout,

    #[error("Channel Send Error: {0}")]
    ChannelSend(String),

    #[error(transparent)]
    ChannelRecv(#[from] async_channel::RecvError),

    #[cfg(feature = "ws")]
    #[error("Ws Error: {0}")]
    WsError(#[from] async_tungstenite::tungstenite::Error),

    #[cfg(feature = "tls")]
    #[error("Tls Error: {0}")]
    Rustls(#[from] karyon_async_rustls::rustls::Error),

    #[cfg(feature = "tls")]
    #[error("Invalid DNS Name: {0}")]
    InvalidDnsNameError(#[from] rustls_pki_types::InvalidDnsNameError),

    #[error(transparent)]
    KaryonCore(#[from] karyon_core::Error),
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(error: async_channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}
