use std::sync::Arc;

use clap::Parser;
use tokio::{runtime::Runtime, sync::mpsc::unbounded_channel};

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

    /// Optional list of peers to dial directly.
    #[arg(short = 'p')]
    dial_peers: Vec<Endpoint>,

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
        dial_peers: cli.dial_peers,
        bootstrap_peers: cli.bootstrap_peers,
        ..Default::default()
    };

    // Create a new tokio runtime
    let rt = Arc::new(Runtime::new().expect("Create tokio runtime"));

    // Create a new Node
    let node = Node::new(&key_pair, config, rt.clone().into());

    let (ctrlc_s, mut ctrlc_r) = unbounded_channel();
    let handle = move || ctrlc_s.send(()).expect("Send ctrlc signal");
    ctrlc::set_handler(handle).expect("ctrlc set handler");

    rt.block_on(async {
        // Run the node
        node.run().await.expect("Run the node");

        // Wait for ctrlc signal
        ctrlc_r.recv().await.expect("Receive ctrlc signal");

        // Shutdown the node
        node.shutdown().await;
    });
}
