use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

/// Represents karyons's p2p Error.
#[derive(ThisError, Debug)]
pub enum Error {
    #[error("IO Error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Unsupported protocol error: {0}")]
    UnsupportedProtocol(String),

    #[error("Invalid message error: {0}")]
    InvalidMsg(String),

    #[error("Parse error: {0}")]
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

    #[error("Channel Receive Error: {0}")]
    ChannelRecv(String),

    #[error("CORE::ERROR : {0}")]
    KaryonsCore(#[from] karyons_core::error::Error),

    #[error("NET::ERROR : {0}")]
    KaryonsNet(#[from] karyons_net::NetError),
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

impl From<std::num::ParseIntError> for Error {
    fn from(error: std::num::ParseIntError) -> Self {
        Error::ParseError(error.to_string())
    }
}

impl From<std::num::ParseFloatError> for Error {
    fn from(error: std::num::ParseFloatError) -> Self {
        Error::ParseError(error.to_string())
    }
}

impl From<semver::Error> for Error {
    fn from(error: semver::Error) -> Self {
        Error::ParseError(error.to_string())
    }
}
