use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Try From Endpoint Error")]
    TryFromEndpoint,

    #[error("Unsupported Endpoint: {0}")]
    UnsupportedEndpoint(String),

    #[error("Parse Endpoint Error: {0}")]
    ParseEndpoint(String),

    #[error("Buffer Full: {0}")]
    BufferFull(String),

    #[error("Missing Config: {0}")]
    MissingConfig(String),

    #[error("SOCKS5 Error: {0}")]
    Socks5(String),

    #[error("TLS Config Error: {0}")]
    TlsConfig(String),

    #[error("QUIC Config Error: {0}")]
    QuicConfigError(String),

    #[error("Connection Closed")]
    ConnectionClosed,

    #[cfg(feature = "ws")]
    #[error("WebSocket Error: {0}")]
    WsError(#[from] Box<async_tungstenite::tungstenite::Error>),

    #[cfg(feature = "tls")]
    #[error("Invalid DNS Name: {0}")]
    InvalidDnsNameError(#[from] rustls_pki_types::InvalidDnsNameError),

    #[cfg(feature = "quic")]
    #[error("QUIC Connection Error: {0}")]
    QuicConnection(#[from] quinn::ConnectionError),

    #[cfg(feature = "quic")]
    #[error("QUIC Connect Error: {0}")]
    QuicConnect(#[from] quinn::ConnectError),

    #[error(transparent)]
    BincodeDecode(#[from] bincode::error::DecodeError),

    #[error(transparent)]
    BincodeEncode(#[from] bincode::error::EncodeError),

    #[error(transparent)]
    KaryonCore(#[from] karyon_core::Error),
}
