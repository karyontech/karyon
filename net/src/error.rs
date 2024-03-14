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

    #[error("invalid endpoint {0}")]
    InvalidEndpoint(String),

    #[error("Parse endpoint error {0}")]
    ParseEndpoint(String),

    #[error("Timeout Error")]
    Timeout,

    #[error("Channel Send Error: {0}")]
    ChannelSend(String),

    #[error(transparent)]
    ChannelRecv(#[from] smol::channel::RecvError),

    #[error("Ws Error: {0}")]
    WsError(#[from] async_tungstenite::tungstenite::Error),

    #[error("Tls Error: {0}")]
    Rustls(#[from] futures_rustls::rustls::Error),

    #[error("Invalid DNS Name: {0}")]
    InvalidDnsNameError(#[from] futures_rustls::pki_types::InvalidDnsNameError),

    #[error(transparent)]
    KaryonCore(#[from] karyon_core::error::Error),
}

impl<T> From<smol::channel::SendError<T>> for Error {
    fn from(error: smol::channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}
