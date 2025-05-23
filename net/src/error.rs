use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Try From Endpoint Error")]
    TryFromEndpoint,

    #[error("Unsupported Endpoint {0}")]
    UnsupportedEndpoint(String),

    #[error("Parse Endpoint Error {0}")]
    ParseEndpoint(String),

    #[cfg(feature = "ws")]
    #[error("Ws Error: {0}")]
    WsError(#[from] Box<async_tungstenite::tungstenite::Error>),

    #[cfg(feature = "tls")]
    #[error("Invalid DNS Name: {0}")]
    InvalidDnsNameError(#[from] rustls_pki_types::InvalidDnsNameError),

    #[error(transparent)]
    KaryonCore(#[from] karyon_core::Error),
}
