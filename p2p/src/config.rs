use karyon_net::Endpoint;

use crate::Version;

/// Configuration for the p2p network.
///
/// `peer_endpoints` are static peers dialed directly (use for fixed
/// topologies). `bootstrap_peers` are seeds for the default Kademlia
/// discovery; the DHT grows the connection set from there. Custom
/// `Discovery` implementations may interpret `bootstrap_peers`
/// differently or ignore it. Pick one, or combine.
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
///     discovery_endpoints: vec![
///         "tcp://0.0.0.0:7000".parse().unwrap(),
///         "udp://0.0.0.0:7000".parse().unwrap(),
///     ],
///     bootstrap_peers: vec![
///         "tcp://seed.example.com:7000".parse().unwrap(),
///     ],
///     ..Config::default()
/// };
/// ```
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
    //
    // Discovery is pluggable via the `Discovery` trait. The default
    // implementation is `KademliaDiscovery`, which is what the fields
    // below describe. Custom implementations may interpret these fields
    // differently or ignore them entirely.
    //
    /// A list of bootstrap peers for the seeding process.
    pub bootstrap_peers: Vec<Endpoint>,
    /// Endpoints to listen on for incoming peer connections.
    /// e.g. [tcp://0.0.0.0:8000, quic://0.0.0.0:9000]
    pub listen_endpoints: Vec<Endpoint>,
    /// Endpoints used by the discovery service (Kademlia by default).
    ///
    /// Kademlia needs two sockets that serve different roles:
    /// - one stream endpoint for the lookup service
    ///   (`tcp://`, `tls://`, or `quic://` when the `quic` feature is on).
    ///   Handles short-lived FIND_NODE / Ping queries from other peers.
    /// - one `udp://` endpoint for the refresh service.
    ///   Handles UDP liveness pings against routing-table entries.
    ///
    /// Either provide both (one of each kind) or leave the vector empty
    /// to disable Kademlia-style DHT discovery and rely on
    /// `peer_endpoints` + `bootstrap_peers` for static peering.
    ///
    /// e.g. [tcp://0.0.0.0:7000, udp://0.0.0.0:7000]
    pub discovery_endpoints: Vec<Endpoint>,
    /// A list of endpoints representing peers that the `Discovery` will
    /// manually connect to.
    pub peer_endpoints: Vec<Endpoint>,
    /// The number of available inbound slots for incoming connections.
    pub inbound_slots: usize,
    /// The number of available outbound slots for outgoing connections.
    pub outbound_slots: usize,
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
            listen_endpoints: vec![],
            discovery_endpoints: vec![],
            peer_endpoints: vec![],
            inbound_slots: 12,
            outbound_slots: 12,
            max_connect_retries: 3,
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
