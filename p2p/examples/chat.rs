use std::{io, sync::Arc};

use async_trait::async_trait;
use blocking::unblock;
use clap::Parser;
use tokio::{runtime::Runtime, spawn, sync::mpsc::unbounded_channel};

use karyon_p2p::{
    endpoint::Endpoint,
    keypair::{KeyPair, KeyPairType},
    protocol::{PeerConn, Protocol, ProtocolID},
    Config, Error, Node, Version,
};

async fn read_line_async() -> Result<String, io::Error> {
    unblock(|| {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        Ok(input)
    })
    .await
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Optional list of bootstrap peers to start the seeding process.
    #[arg(short)]
    bootstrap_peers: Vec<Endpoint>,

    /// Optional list of peers to dial directly.
    #[arg(short = 'p')]
    dial_peers: Vec<Endpoint>,

    /// Endpoints for accepting incoming connections.
    #[arg(short)]
    listen_endpoints: Vec<Endpoint>,

    /// Discovery endpoints (e.g. tcp://0.0.0.0:7000 udp://0.0.0.0:7000).
    #[arg(short)]
    discovery_endpoints: Vec<Endpoint>,

    /// Username
    #[arg(long)]
    username: String,
}

pub struct ChatProtocol {
    username: String,
    peer: PeerConn,
}

impl ChatProtocol {
    fn new(username: &str, peer: PeerConn) -> Self {
        Self {
            peer,
            username: username.to_string(),
        }
    }
}

#[async_trait]
impl Protocol for ChatProtocol {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        let task = spawn({
            let this = self.clone();
            async move {
                loop {
                    let input = read_line_async().await.expect("Read line from stdin");
                    let msg = format!("> {}: {}", this.username, input.trim());
                    this.peer.broadcast(msg.into_bytes()).await;
                }
            }
        });

        loop {
            match self.peer.recv().await {
                Ok(bytes) => {
                    let msg = String::from_utf8(bytes).expect("Convert received bytes to string");
                    println!("{msg}");
                }
                Err(Error::PeerShutdown) => break,
                Err(e) => return Err(e),
            }
        }

        task.abort();
        Ok(())
    }

    fn version() -> Result<Version, Error> {
        "0.1.0, 0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "CHAT".into()
    }
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    // Generate a random keypair; the PeerID is derived from its public key.
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
        let username = cli.username;

        // Attach the ChatProtocol
        node.attach_protocol(move |peer| ChatProtocol::new(&username, peer))
            .await
            .expect("Attach chat protocol to the p2p node");

        // Run the node
        node.run().await.expect("Run the node");

        // Wait for ctrlc signal
        ctrlc_r.recv().await.expect("Receive ctrlc signal");

        // Shutdown the node
        node.shutdown().await;
    });
}
