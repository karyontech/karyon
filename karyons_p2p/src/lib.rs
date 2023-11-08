mod backend;
mod config;
mod discovery;
mod error;
mod io_codec;
mod message;
mod net;
mod peer;
mod peer_pool;
mod protocols;
mod routing_table;
mod utils;

/// Responsible for network and system monitoring.
/// [`Read More`](./monitor/struct.Monitor.html)
pub mod monitor;
/// Defines the protocol trait.
/// [`Read More`](./protocol/trait.Protocol.html)
pub mod protocol;

pub use backend::{ArcBackend, Backend};
pub use config::Config;
pub use error::Error as P2pError;
pub use peer::{ArcPeer, PeerID};
pub use utils::Version;

use error::{Error, Result};
