use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

/// Represents karyon's p2p Error.
#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Unsupported Protocol: {0}")]
    UnsupportedProtocol(String),

    #[error("Unsupported Endpoint: {0}")]
    UnsupportedEndpoint(String),

    #[error("Invalid Endpoint: {0}")]
    InvalidEndpoint(String),

    #[error("PeerID Try From PublicKey")]
    PeerIDTryFromPublicKey,

    #[error("PeerID Try From String")]
    PeerIDTryFromString,

    #[error("Invalid Message: {0}")]
    InvalidMsg(String),

    #[error("Incompatible Peer")]
    IncompatiblePeer,

    #[error("Incompatible Version: {0}")]
    IncompatibleVersion(String),

    #[error("Timeout")]
    Timeout,

    #[error("Parse Error: {0}")]
    ParseError(String),

    #[error("Config Error: {0}")]
    Config(String),

    #[error("Peer Shutdown")]
    PeerShutdown,

    #[error("Invalid Pong Msg")]
    InvalidPongMsg,

    #[error("Peer Already Connected")]
    PeerAlreadyConnected,

    #[error("Peer Not Found: {0}")]
    PeerNotFound(String),

    #[error("Discovery Error: {0}")]
    Discovery(String),

    #[error("Lookup Error: {0}")]
    Lookup(String),

    #[error("Channel Send Error: {0}")]
    ChannelSend(String),

    #[error(transparent)]
    ChannelRecv(#[from] async_channel::RecvError),

    #[error(transparent)]
    ParseIntError(#[from] std::num::ParseIntError),

    #[error(transparent)]
    Base64Decode(#[from] base64::DecodeError),

    #[error(transparent)]
    BincodeDecode(#[from] bincode::error::DecodeError),

    #[error(transparent)]
    BincodeEncode(#[from] bincode::error::EncodeError),

    #[error(transparent)]
    ParseFloatError(#[from] std::num::ParseFloatError),

    #[error(transparent)]
    SemverError(#[from] semver::Error),

    #[error(transparent)]
    Yasna(#[from] yasna::ASN1Error),

    #[error(transparent)]
    X509Parser(#[from] x509_parser::error::X509Error),

    #[error(transparent)]
    Rcgen(#[from] rcgen::Error),

    #[cfg(feature = "smol")]
    #[error("TLS Error: {0}")]
    Rustls(#[from] futures_rustls::rustls::Error),

    #[cfg(feature = "tokio")]
    #[error("TLS Error: {0}")]
    Rustls(#[from] tokio_rustls::rustls::Error),

    #[error("Invalid DNS Name: {0}")]
    InvalidDnsNameError(#[from] rustls_pki_types::InvalidDnsNameError),

    #[error(transparent)]
    KaryonCore(#[from] karyon_core::error::Error),

    #[error(transparent)]
    KaryonNet(#[from] karyon_net::Error),

    #[error(transparent)]
    KaryonEventEmitter(#[from] karyon_eventemitter::error::Error),
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(error: async_channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}
