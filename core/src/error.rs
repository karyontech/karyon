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

    #[cfg(feature = "crypto")]
    #[error(transparent)]
    Ed25519(#[from] ed25519_dalek::ed25519::Error),

    #[cfg(feature = "tokio")]
    #[error(transparent)]
    TokioJoinError(#[from] tokio::task::JoinError),
}
