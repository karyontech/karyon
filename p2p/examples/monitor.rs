mod shared;

use std::sync::Arc;

use clap::Parser;
use smol::{channel, Executor};

use karyons_net::{Endpoint, Port};

use karyons_p2p::{Backend, Config, PeerID};

use shared::run_executor;

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

    /// Optional user id
    #[arg(long)]
    userid: Option<String>,
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let peer_id = match cli.userid {
        Some(userid) => PeerID::new(userid.as_bytes()),
        None => PeerID::random(),
    };

    // Create the configuration for the backend.
    let config = Config {
        listen_endpoint: cli.listen_endpoint,
        peer_endpoints: cli.peer_endpoints,
        bootstrap_peers: cli.bootstrap_peers,
        discovery_port: cli.discovery_port.unwrap_or(0),
        ..Default::default()
    };

    // Create a new Executor
    let ex = Arc::new(Executor::new());

    // Create a new Backend
    let backend = Backend::new(peer_id, config, ex.clone());

    let (ctrlc_s, ctrlc_r) = channel::unbounded();
    let handle = move || ctrlc_s.try_send(()).unwrap();
    ctrlc::set_handler(handle).unwrap();

    let exc = ex.clone();
    run_executor(
        async {
            let monitor = backend.monitor().await;

            let monitor_task = exc.spawn(async move {
                loop {
                    let event = monitor.recv().await.unwrap();
                    println!("{}", event);
                }
            });

            // Run the backend
            backend.run().await.unwrap();

            // Wait for ctrlc signal
            ctrlc_r.recv().await.unwrap();

            // Shutdown the backend
            backend.shutdown().await;

            monitor_task.cancel().await;
        },
        ex,
    );
}
