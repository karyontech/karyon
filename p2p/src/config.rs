use karyon_net::{Endpoint, Port};

use crate::Version;

/// the Configuration for the P2P network.
pub struct Config {
    /// Represents the network version.
    pub version: Version,

    /// Enable monitor
    pub enable_monitor: bool,

    /////////////////
    // PeerPool
    ////////////////
    /// Timeout duration for the handshake with new peers, in seconds.
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
    // DISCOVERY
    ////////////////
    /// A list of bootstrap peers for the seeding process.
    pub bootstrap_peers: Vec<Endpoint>,
    /// An optional listening endpoint to accept incoming connections.
    pub listen_endpoint: Option<Endpoint>,
    /// A list of endpoints representing peers that the `Discovery` will
    /// manually connect to.
    pub peer_endpoints: Vec<Endpoint>,
    /// The number of available inbound slots for incoming connections.
    pub inbound_slots: usize,
    /// The number of available outbound slots for outgoing connections.
    pub outbound_slots: usize,
    /// TCP/UDP port for lookup and refresh processes.
    pub discovery_port: Port,
    /// Time interval, in seconds, at which the Discovery restarts the
    /// seeding process.
    pub seeding_interval: u64,

    /////////////////
    // LOOKUP
    ////////////////
    /// The number of available inbound slots for incoming connections during
    /// the lookup process.
    pub lookup_inbound_slots: usize,
    /// The number of available outbound slots for outgoing connections during
    /// the lookup process.
    pub lookup_outbound_slots: usize,
    /// Timeout duration for a peer response during the lookup process, in
    /// seconds.
    pub lookup_response_timeout: u64,
    /// Maximum allowable time for a live connection with a peer during the
    /// lookup process, in seconds.
    pub lookup_connection_lifespan: u64,
    /// The maximum number of retries for outbound connection establishment
    /// during the lookup process.
    pub lookup_connect_retries: usize,

    /////////////////
    // REFRESH
    ////////////////
    /// Interval at which the table refreshes its entries, in seconds.
    pub refresh_interval: u64,
    /// Timeout duration for a peer response during the table refresh process,
    /// in seconds.
    pub refresh_response_timeout: u64,
    /// The maximum number of retries for outbound connection establishment
    /// during the refresh process.
    pub refresh_connect_retries: usize,

    /// Enables TLS for all connections.
    pub enable_tls: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: "0.1.0".parse().unwrap(),

            enable_monitor: false,

            handshake_timeout: 2,
            ping_interval: 20,
            ping_timeout: 2,

            bootstrap_peers: vec![],
            listen_endpoint: None,
            peer_endpoints: vec![],
            inbound_slots: 12,
            outbound_slots: 12,
            max_connect_retries: 3,
            discovery_port: 0,
            seeding_interval: 60,

            lookup_inbound_slots: 20,
            lookup_outbound_slots: 20,
            lookup_response_timeout: 1,
            lookup_connection_lifespan: 3,
            lookup_connect_retries: 3,

            refresh_interval: 1800,
            refresh_response_timeout: 1,
            refresh_connect_retries: 3,

            enable_tls: false,
        }
    }
}
