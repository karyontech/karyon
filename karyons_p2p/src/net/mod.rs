mod connection_queue;
mod connector;
mod listener;
mod slots;

pub use connection_queue::ConnQueue;
pub use connector::Connector;
pub use listener::Listener;
pub use slots::ConnectionSlots;

use std::fmt;

/// Defines the direction of a network connection.
#[derive(Clone, Debug)]
pub enum ConnDirection {
    Inbound,
    Outbound,
}

impl fmt::Display for ConnDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConnDirection::Inbound => write!(f, "Inbound"),
            ConnDirection::Outbound => write!(f, "Outbound"),
        }
    }
}
