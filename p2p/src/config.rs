use std::sync::Arc;

use karyon_net::Endpoint;

use crate::{
    Version,
    access_control::{AccessControl, AllowAll},
};

/// Configuration for the p2p network.
///
/// `dial_peers` are static peers dialed directly. To find more peers, run
/// the node with a discovery (see `Node::with_discovery`), which has its
/// own configuration.
///
/// # Example
///
/// ```
/// use karyon_p2p::Config;
///
/// let config = Config {
///     listen_endpoints: vec![
///         "tcp://0.0.0.0:8000".parse().unwrap(),
///     ],
///     dial_peers: vec![
///         "tcp://203.0.113.1:8000".parse().unwrap(),
///     ],
///     ..Config::default()
/// };
/// ```
pub struct Config {
    /// Represents the network version.
    pub version: Version,

    /// Enable monitor
    pub enable_monitor: bool,

    /// Policy deciding who this node talks to. Defaults to `AllowAll`.
    pub access_control: Arc<dyn AccessControl>,

    /////////////////
    // PeerPool
    ////////////////
    /// Timeout duration for the handshake with new peers, in seconds.
    /// Applied twice on an inbound connection: once for the transport
    /// handshake (TLS, or the first QUIC stream), then once for the
    /// protocol handshake.
    pub handshake_timeout: u64,
    /// Interval at which the ping protocol sends ping messages to a peer to
    /// maintain connections, in seconds.
    pub ping_interval: u64,
    /// Timeout duration for receiving the pong message corresponding to the
    /// sent ping message, in seconds.
    pub ping_timeout: u64,
    /// The maximum number of retries for outbound connection establishment.
    pub max_connect_retries: usize,

    /////////////////
    // CONNECTIONS
    ////////////////
    /// Endpoints to listen on for incoming peer connections.
    /// e.g. [tcp://0.0.0.0:8000, quic://0.0.0.0:9000]
    pub listen_endpoints: Vec<Endpoint>,
    /// Peers the node dials once at start, with any discovery.
    pub dial_peers: Vec<Endpoint>,
    /// The number of available inbound slots for incoming connections.
    pub inbound_slots: usize,
    /// The number of available outbound slots for outgoing connections.
    pub outbound_slots: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: "0.2.0".parse().unwrap(),

            enable_monitor: false,
            access_control: Arc::new(AllowAll),

            handshake_timeout: 4,
            ping_interval: 20,
            ping_timeout: 2,

            listen_endpoints: vec![],
            dial_peers: vec![],
            inbound_slots: 12,
            outbound_slots: 12,
            max_connect_retries: 3,
        }
    }
}
