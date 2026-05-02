use std::sync::Arc;

use clap::Parser;
use smol::{channel, Executor};

use karyon_core::testing::run_executor;
use karyon_p2p::{
    endpoint::Endpoint,
    keypair::{KeyPair, KeyPairType},
    Config, Node,
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Optional list of bootstrap peers to start the seeding process.
    #[arg(short)]
    bootstrap_peers: Vec<Endpoint>,

    /// Optional list of peer endpoints for manual connections.
    #[arg(short)]
    peer_endpoints: Vec<Endpoint>,

    /// Endpoints for accepting incoming connections (e.g. tcp://0.0.0.0:8000).
    #[arg(short)]
    listen_endpoints: Vec<Endpoint>,

    /// Discovery endpoints (e.g. tcp://0.0.0.0:7000 udp://0.0.0.0:7000).
    #[arg(short)]
    discovery_endpoints: Vec<Endpoint>,
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);

    // Create the configuration for the node.
    let config = Config {
        listen_endpoints: cli.listen_endpoints,
        discovery_endpoints: cli.discovery_endpoints,
        peer_endpoints: cli.peer_endpoints,
        bootstrap_peers: cli.bootstrap_peers,
        ..Default::default()
    };

    // Create a new Executor
    let ex = Arc::new(Executor::new());

    // Create a new Node
    let node = Node::new(&key_pair, config, ex.clone().into());

    let (ctrlc_s, ctrlc_r) = channel::unbounded();
    let handle = move || ctrlc_s.try_send(()).expect("Send ctrlc signal");
    ctrlc::set_handler(handle).expect("ctrlc set handler");

    run_executor(
        async {
            // Run the node
            node.run().await.expect("Run the node");

            // Wait for ctrlc signal
            ctrlc_r.recv().await.expect("Receive ctrlc signal");

            // Shutdown the node
            node.shutdown().await;
        },
        ex,
    );
}
