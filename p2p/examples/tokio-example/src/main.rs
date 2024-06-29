use std::sync::Arc;

use clap::Parser;

use karyon_p2p::{
    endpoint::{Endpoint, Port},
    keypair::{KeyPair, KeyPairType},
    Backend, Config,
};

/// Returns an estimate of the default amount of parallelism a program should use.
/// see `std::thread::available_parallelism`
pub fn available_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Optional list of bootstrap peers to start the seeding process.
    #[arg(short)]
    bootstrap_peers: Vec<Endpoint>,

    /// Optional list of peer endpoints for manual connections.
    #[arg(short)]
    peer_endpoints: Vec<Endpoint>,

    /// Optional endpoint for accepting incoming connections.
    #[arg(short)]
    listen_endpoint: Option<Endpoint>,

    /// Optional TCP/UDP port for the discovery service.
    #[arg(short)]
    discovery_port: Option<Port>,
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);

    // Create the configuration for the backend.
    let config = Config {
        listen_endpoint: cli.listen_endpoint,
        peer_endpoints: cli.peer_endpoints,
        bootstrap_peers: cli.bootstrap_peers,
        discovery_port: cli.discovery_port.unwrap_or(0),
        ..Default::default()
    };

    // Create a new tokio runtime
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(available_parallelism())
            .enable_all()
            .build()
            .expect("Build a new tokio runtime"),
    );

    // Create a new Backend
    let backend = Backend::new(&key_pair, config, rt.clone().into());

    let (ctrlc_s, ctrlc_r) = async_channel::unbounded();
    let handle = move || ctrlc_s.try_send(()).expect("Send ctrlc signal");
    ctrlc::set_handler(handle).expect("ctrlc set handler");

    rt.block_on(async {
        // Run the backend
        backend.run().await.expect("Run the backend");

        // Wait for ctrlc signal
        ctrlc_r.recv().await.expect("Receive ctrlc signal");

        // Shutdown the backend
        backend.shutdown().await;
    });
}
