use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

/// Represents karyon's p2p Error.
#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Unsupported protocol error: {0}")]
    UnsupportedProtocol(String),

    #[error("Unsupported Endpoint: {0}")]
    UnsupportedEndpoint(String),

    #[error("PeerID try from PublicKey Error")]
    PeerIDTryFromPublicKey,

    #[error("PeerID try from String Error")]
    PeerIDTryFromString,

    #[error("Invalid message error: {0}")]
    InvalidMsg(String),

    #[error("Incompatible Peer")]
    IncompatiblePeer,

    #[error(transparent)]
    ParseIntError(#[from] std::num::ParseIntError),

    #[error(transparent)]
    ParseIntError2(#[from] base64::DecodeError),

    #[error(transparent)]
    ParseFloatError(#[from] std::num::ParseFloatError),

    #[error(transparent)]
    SemverError(#[from] semver::Error),

    #[error("Parse Error: {0}")]
    ParseError(String),

    #[error("Incompatible version error: {0}")]
    IncompatibleVersion(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Peer shutdown")]
    PeerShutdown,

    #[error("Invalid Pong Msg")]
    InvalidPongMsg,

    #[error("Discovery error: {0}")]
    Discovery(&'static str),

    #[error("Lookup error: {0}")]
    Lookup(&'static str),

    #[error("Peer already connected")]
    PeerAlreadyConnected,

    #[error("Yasna Error: {0}")]
    Yasna(#[from] yasna::ASN1Error),

    #[error("X509 Parser Error: {0}")]
    X509Parser(#[from] x509_parser::error::X509Error),

    #[error("Rcgen Error: {0}")]
    Rcgen(#[from] rcgen::Error),

    #[cfg(feature = "smol")]
    #[error("Tls Error: {0}")]
    Rustls(#[from] futures_rustls::rustls::Error),

    #[cfg(feature = "tokio")]
    #[error("Tls Error: {0}")]
    Rustls(#[from] tokio_rustls::rustls::Error),

    #[error("Invalid DNS Name: {0}")]
    InvalidDnsNameError(#[from] rustls_pki_types::InvalidDnsNameError),

    #[error("Channel Send Error: {0}")]
    ChannelSend(String),

    #[error(transparent)]
    ChannelRecv(#[from] async_channel::RecvError),

    #[error(transparent)]
    KaryonCore(#[from] karyon_core::error::Error),

    #[error(transparent)]
    KaryonNet(#[from] karyon_net::Error),

    #[error("Other Error: {0}")]
    Other(String),
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(error: async_channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}
