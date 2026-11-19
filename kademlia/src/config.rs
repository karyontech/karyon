use std::net::IpAddr;

use karyon_net::Endpoint;

/// Configuration of the Kademlia discovery.
///
/// The node settings it also uses (`listen_endpoints`, `version`,
/// `access_control`, `handshake_timeout`) come from the node's
/// `karyon_p2p::Config`.
///
/// # Example
///
/// ```
/// use karyon_kademlia::KademliaConfig;
///
/// let config = KademliaConfig {
///     discovery_endpoints: vec![
///         "tcp://0.0.0.0:7000".parse().unwrap(),
///         "udp://0.0.0.0:7000".parse().unwrap(),
///     ],
///     bootstrap_peers: vec![
///         "tcp://seed.example.com:7000".parse().unwrap(),
///     ],
///     ..KademliaConfig::default()
/// };
/// ```
#[derive(Clone, Debug)]
pub struct KademliaConfig {
    /// Endpoints used by the discovery service.
    ///
    /// Kademlia needs two sockets that serve different roles:
    /// - one stream endpoint for the lookup service
    ///   (`tcp://`, `tls://`, or `quic://` when the `quic` feature is on).
    ///   Handles short-lived FIND_NODE / Ping queries from other peers.
    /// - one `udp://` endpoint for the refresh service.
    ///   Handles UDP liveness pings against routing-table entries.
    ///
    /// Either provide both (one of each kind) or leave the vector empty
    /// to only look up the bootstrap peers.
    ///
    /// e.g. [tcp://0.0.0.0:7000, udp://0.0.0.0:7000]
    pub discovery_endpoints: Vec<Endpoint>,
    /// A list of bootstrap peers for the seeding process.
    pub bootstrap_peers: Vec<Endpoint>,
    /// This node's public IP, if known (e.g. learned with karyon_stun).
    ///
    /// When set, discovery advertises it in place of the IP of every listen
    /// and discovery endpoint of the same family. Ports are kept as
    /// configured, so they must be reachable from outside (port
    /// forwarding). `None` (the default) advertises endpoints as
    /// configured.
    pub public_ip: Option<IpAddr>,
    /// Time interval, in seconds, at which the discovery restarts the
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
}

impl Default for KademliaConfig {
    fn default() -> Self {
        Self {
            discovery_endpoints: vec![],
            bootstrap_peers: vec![],
            public_ip: None,
            seeding_interval: 60,

            lookup_inbound_slots: 20,
            lookup_outbound_slots: 20,
            lookup_response_timeout: 1,
            lookup_connection_lifespan: 3,
            lookup_connect_retries: 3,

            refresh_interval: 1800,
            refresh_response_timeout: 1,
            refresh_connect_retries: 3,
        }
    }
}
