use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

/// Represents karyons's p2p Error.
#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Unsupported protocol error: {0}")]
    UnsupportedProtocol(String),

    #[error("Invalid message error: {0}")]
    InvalidMsg(String),

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

    #[error("Channel Send Error: {0}")]
    ChannelSend(String),

    #[error(transparent)]
    ChannelRecv(#[from] smol::channel::RecvError),

    #[error(transparent)]
    KaryonsCore(#[from] karyons_core::error::Error),

    #[error(transparent)]
    KaryonsNet(#[from] karyons_net::NetError),
}

impl<T> From<smol::channel::SendError<T>> for Error {
    fn from(error: smol::channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}
