use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

/// Represents karyon's p2p Error.
#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Unsupported protocol error: {0}")]
    UnsupportedProtocol(String),

    #[error("Try from public key Error: {0}")]
    TryFromPublicKey(&'static str),

    #[error("Invalid message error: {0}")]
    InvalidMsg(String),

    #[error("Incompatible Peer")]
    IncompatiblePeer,

    #[error(transparent)]
    ParseIntError(#[from] std::num::ParseIntError),

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

    #[error("Tls Error: {0}")]
    Rustls(#[from] futures_rustls::rustls::Error),

    #[error("Invalid DNS Name: {0}")]
    InvalidDnsNameError(#[from] futures_rustls::pki_types::InvalidDnsNameError),

    #[error("Channel Send Error: {0}")]
    ChannelSend(String),

    #[error(transparent)]
    ChannelRecv(#[from] smol::channel::RecvError),

    #[error(transparent)]
    KaryonCore(#[from] karyon_core::error::Error),

    #[error(transparent)]
    KaryonNet(#[from] karyon_net::NetError),
}

impl<T> From<smol::channel::SendError<T>> for Error {
    fn from(error: smol::channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}
