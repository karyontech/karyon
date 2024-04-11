#[cfg(feature = "smol")]
pub use smol::io::{
    split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf,
};

#[cfg(feature = "tokio")]
pub use tokio::io::{
    split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf,
};
