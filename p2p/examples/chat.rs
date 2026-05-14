use std::{io, sync::Arc};

use async_trait::async_trait;
use blocking::unblock;
use clap::Parser;
use smol::{channel, Executor};

use karyon_core::testing::run_executor;
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

    /// Optional list of peer endpoints for manual connections.
    #[arg(short)]
    peer_endpoints: Vec<Endpoint>,

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
    executor: Arc<Executor<'static>>,
}

impl ChatProtocol {
    fn new(username: &str, peer: PeerConn, executor: Arc<Executor<'static>>) -> Arc<dyn Protocol> {
        Arc::new(Self {
            peer,
            username: username.to_string(),
            executor,
        })
    }
}

#[async_trait]
impl Protocol for ChatProtocol {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        let task = self.executor.spawn({
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

        task.cancel().await;
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

    // Create a PeerID based on the username.
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
        {
            let ex = ex.clone();
            async {
                let username = cli.username;

                // Attach the ChatProtocol
                let c = move |peer| Ok(ChatProtocol::new(&username, peer, ex.clone()));
                node.attach_protocol::<ChatProtocol>(c)
                    .await
                    .expect("Attach chat protocol to the p2p node");

                // Run the node
                node.run().await.expect("Run the node");

                // Wait for ctrlc signal
                ctrlc_r.recv().await.expect("Receive ctrlc signal");

                // Shutdown the node
                node.shutdown().await;
            }
        },
        ex,
    );
}
