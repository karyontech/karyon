use std::io::{Error as IoError, ErrorKind};
use std::result::Result as StdResult;

use thiserror::Error as ThisError;

use karyon_net::Error as NetError;

pub type Result<T> = StdResult<T, Error>;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] IoError),

    #[error(transparent)]
    KaryonNet(#[from] NetError),

    #[error("Invalid Message: {0}")]
    InvalidMsg(String),

    #[error("Timeout")]
    Timeout,

    #[error("Transaction ID Mismatch")]
    TransactionMismatch,

    #[error("No Mapped Address In Response")]
    NoMappedAddress,

    #[error("STUN Error Response: {code} {reason}")]
    ErrorResponse { code: u16, reason: String },

    #[error("Unsupported Endpoint: {0}")]
    UnsupportedEndpoint(String),

    #[error("Missing {0} Config")]
    MissingConfig(&'static str),
}

// Required so `StunCodec` plugs into karyon_net's framing.
impl From<Error> for NetError {
    fn from(err: Error) -> NetError {
        match err {
            Error::KaryonNet(e) => e,
            Error::IO(e) => NetError::IO(e),
            other => NetError::IO(IoError::new(ErrorKind::InvalidData, other.to_string())),
        }
    }
}
