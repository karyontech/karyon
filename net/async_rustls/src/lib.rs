#[cfg(all(feature = "smol", feature = "tokio"))]
compile_error!("Only one async runtime feature should be enabled");

#[cfg(not(any(feature = "smol", feature = "tokio")))]
compile_error!("At least one async runtime feature must be enabled for this crate.");

#[cfg(feature = "smol")]
pub use futures_rustls::{rustls, TlsAcceptor, TlsConnector, TlsStream};

#[cfg(feature = "tokio")]
pub use tokio_rustls::{rustls, TlsAcceptor, TlsConnector, TlsStream};

