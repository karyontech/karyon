use std::sync::Arc;

use karyon_net::Endpoint;

use crate::{
    Version,
    access_control::{AccessControlRef, AllowAll},
    transport::TransportRef,
};

/// `Arc` reference to a `Config`.
pub type ConfigRef = Arc<Config>;

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
    pub access_control: AccessControlRef,

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
    /// A user transport for the endpoints it supports; the others use the
    /// built-in TCP, TLS and QUIC. `None` uses only the built-in ones.
    pub transport: Option<TransportRef>,
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
            transport: None,
        }
    }
}

/// Builds a `Config`, starting from `Config::default()`. The fields stay
/// public, so a struct literal works too.
///
/// # Example
///
/// ```
/// use karyon_p2p::ConfigBuilder;
///
/// let config = ConfigBuilder::new()
///     .with_listen_endpoints(vec!["tcp://0.0.0.0:8000".parse().unwrap()])
///     .with_dial_peers(vec!["tcp://203.0.113.1:8000".parse().unwrap()])
///     .build();
/// ```
#[derive(Default)]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// See `Config::version`.
    pub fn with_version(mut self, value: Version) -> Self {
        self.config.version = value;
        self
    }

    /// See `Config::enable_monitor`.
    pub fn with_monitor(mut self, value: bool) -> Self {
        self.config.enable_monitor = value;
        self
    }

    /// See `Config::access_control`.
    pub fn with_access_control(mut self, value: AccessControlRef) -> Self {
        self.config.access_control = value;
        self
    }

    /// See `Config::handshake_timeout`, in seconds.
    pub fn with_handshake_timeout(mut self, value: u64) -> Self {
        self.config.handshake_timeout = value;
        self
    }

    /// See `Config::ping_interval`, in seconds.
    pub fn with_ping_interval(mut self, value: u64) -> Self {
        self.config.ping_interval = value;
        self
    }

    /// See `Config::ping_timeout`, in seconds.
    pub fn with_ping_timeout(mut self, value: u64) -> Self {
        self.config.ping_timeout = value;
        self
    }

    /// See `Config::max_connect_retries`.
    pub fn with_max_connect_retries(mut self, value: usize) -> Self {
        self.config.max_connect_retries = value;
        self
    }

    /// See `Config::listen_endpoints`.
    pub fn with_listen_endpoints(mut self, value: Vec<Endpoint>) -> Self {
        self.config.listen_endpoints = value;
        self
    }

    /// See `Config::dial_peers`.
    pub fn with_dial_peers(mut self, value: Vec<Endpoint>) -> Self {
        self.config.dial_peers = value;
        self
    }

    /// See `Config::inbound_slots`.
    pub fn with_inbound_slots(mut self, value: usize) -> Self {
        self.config.inbound_slots = value;
        self
    }

    /// See `Config::outbound_slots`.
    pub fn with_outbound_slots(mut self, value: usize) -> Self {
        self.config.outbound_slots = value;
        self
    }

    /// See `Config::transport`.
    pub fn with_transport(mut self, transport: TransportRef) -> Self {
        self.config.transport = Some(transport);
        self
    }

    pub fn build(self) -> Config {
        self.config
    }
}
