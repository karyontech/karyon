use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("TryInto Error: {0}")]
    TryInto(String),

    #[error("Timeout Error")]
    Timeout,

    #[error("Path Not Found Error: {0}")]
    PathNotFound(String),

    #[cfg(feature = "crypto")]
    #[error(transparent)]
    Ed25519(#[from] ed25519_dalek::ed25519::Error),

    #[cfg(feature = "tokio")]
    #[error(transparent)]
    TokioJoinError(#[from] tokio::task::JoinError),

    #[error(transparent)]
    BincodeDecode(#[from] bincode::error::DecodeError),

    #[error(transparent)]
    BincodeEncode(#[from] bincode::error::EncodeError),
}
