mod shared;

use std::sync::Arc;

use clap::Parser;
use log::error;
use smol::{channel, Executor};

use karyon_p2p::{
    endpoint::{Endpoint, Port},
    keypair::{KeyPair, KeyPairType},
    ArcBackend, Backend, Config,
};

use karyon_jsonrpc::{rpc_impl, rpc_pubsub_impl, ArcChannel, Server, SubscriptionID};

use shared::run_executor;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Optional list of bootstrap peers to start the seeding process.
    #[arg(short)]
    bootstrap_peers: Vec<Endpoint>,

    /// RPC server endpoint.
    #[arg(short)]
    rpc_endpoint: Endpoint,

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

struct MonitorRPC {
    backend: ArcBackend,
}

#[rpc_impl]
impl MonitorRPC {
    async fn peer_id(
        &self,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, karyon_jsonrpc::Error> {
        Ok(serde_json::json!(self.backend.peer_id().to_string()))
    }

    async fn inbound_connection(
        &self,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, karyon_jsonrpc::Error> {
        Ok(serde_json::json!(self.backend.inbound_slots()))
    }

    async fn outbound_connection(
        &self,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, karyon_jsonrpc::Error> {
        Ok(serde_json::json!(self.backend.inbound_slots()))
    }
}

#[rpc_pubsub_impl]
impl MonitorRPC {
    async fn conn_subscribe(
        &self,
        chan: ArcChannel,
        method: String,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, karyon_jsonrpc::Error> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id.clone();
        let conn_events = self.backend.monitor().conn_events().await;
        smol::spawn(async move {
            loop {
                let _event = conn_events.recv().await;
                if let Err(err) = sub.notify(serde_json::json!("event")).await {
                    error!("Failed to notify: {err}");
                    break;
                }
            }
        })
        .detach();

        Ok(serde_json::json!(sub_id))
    }

    async fn conn_unsubscribe(
        &self,
        chan: ArcChannel,
        _method: String,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, karyon_jsonrpc::Error> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        chan.remove_subscription(&sub_id).await;
        Ok(serde_json::json!(true))
    }
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
        enable_monitor: true,
        ..Default::default()
    };

    // Create a new Executor
    let ex = Arc::new(Executor::new());

    // Create a new Backend
    let backend = Backend::new(&key_pair, config, ex.clone().into());

    let (ctrlc_s, ctrlc_r) = channel::unbounded();
    let handle = move || ctrlc_s.try_send(()).unwrap();
    ctrlc::set_handler(handle).unwrap();

    let exc = ex.clone();
    run_executor(
        async {
            // RPC service
            let service = Arc::new(MonitorRPC {
                backend: backend.clone(),
            });

            // Create rpc server
            let server = Server::builder(cli.rpc_endpoint)
                .expect("Create server builder")
                .service(service.clone())
                .pubsub_service(service)
                .build_with_executor(exc.clone().into())
                .await
                .expect("Build rpc server");

            // Run the backend
            backend.run().await.expect("Run p2p backend");

            // Run the RPC server
            server.start().await;

            // Wait for ctrlc signal
            ctrlc_r.recv().await.expect("Wait for ctrlc signal");

            // Shutdown the backend
            backend.shutdown().await;

            // Shutdown the RPC server
            server.shutdown().await;
        },
        ex,
    );
}
