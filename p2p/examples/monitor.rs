mod shared;

use std::{sync::Arc, time::Duration};

use clap::Parser;
use log::error;
use serde::{Deserialize, Serialize};
use smol::{channel, Executor};

use karyon_p2p::{
    endpoint::{Endpoint, Port},
    keypair::{KeyPair, KeyPairType},
    monitor::{ConnEvent, DiscoveryEvent, PeerPoolEvent},
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
                let event = conn_events.recv().await.unwrap();
                let event: ConnEventJson = event.into();
                if let Err(err) = sub.notify(serde_json::json!(event)).await {
                    error!("Failed to notify: {err}");
                    break;
                }
            }
        })
        .detach();

        Ok(serde_json::json!(sub_id))
    }

    async fn peer_pool_subscribe(
        &self,
        chan: ArcChannel,
        method: String,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, karyon_jsonrpc::Error> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id.clone();
        let peer_pool_events = self.backend.monitor().peer_pool_events().await;
        smol::spawn(async move {
            loop {
                let event = peer_pool_events.recv().await.unwrap();
                let event: PeerPoolEventJson = event.into();
                if let Err(err) = sub.notify(serde_json::json!(event)).await {
                    error!("Failed to notify: {err}");
                    break;
                }
            }
        })
        .detach();

        Ok(serde_json::json!(sub_id))
    }

    async fn discovery_subscribe(
        &self,
        chan: ArcChannel,
        method: String,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, karyon_jsonrpc::Error> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id.clone();
        let discovery_events = self.backend.monitor().discovery_events().await;
        smol::spawn(async move {
            loop {
                let event = discovery_events.recv().await.unwrap();
                let event: DiscoveryEventJson = event.into();
                if let Err(err) = sub.notify(serde_json::json!(event)).await {
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

    async fn peer_pool_unsubscribe(
        &self,
        chan: ArcChannel,
        _method: String,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, karyon_jsonrpc::Error> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        chan.remove_subscription(&sub_id).await;
        Ok(serde_json::json!(true))
    }

    async fn discovery_unsubscribe(
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

            // Run the RPC server
            server.start().await;

            // Run the backend
            backend.run().await.expect("Run p2p backend");

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

#[derive(Debug, Serialize, Deserialize)]
struct ConnEventJson {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PeerPoolEventJson {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DiscoveryEventJson {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint: Option<String>,
}

impl From<ConnEvent> for ConnEventJson {
    fn from(item: ConnEvent) -> Self {
        match item {
            ConnEvent::Connected(ref e) => ConnEventJson {
                name: "Connected".into(),
                endpoint: Some(e.to_string()),
            },
            ConnEvent::Disconnected(e) => ConnEventJson {
                name: "Disconnected".into(),
                endpoint: Some(e.to_string()),
            },
            ConnEvent::ConnectFailed(e) => ConnEventJson {
                name: "ConnectFailed".into(),
                endpoint: Some(e.to_string()),
            },
            ConnEvent::ConnectRetried(e) => ConnEventJson {
                name: "ConnectRetried".into(),
                endpoint: Some(e.to_string()),
            },
            ConnEvent::Accepted(e) => ConnEventJson {
                name: "Accepted".into(),
                endpoint: Some(e.to_string()),
            },
            ConnEvent::AcceptFailed => ConnEventJson {
                name: "AcceptFailed".into(),
                endpoint: None,
            },
            ConnEvent::Listening(e) => ConnEventJson {
                name: "Listening".into(),
                endpoint: Some(e.to_string()),
            },
            ConnEvent::ListenFailed(e) => ConnEventJson {
                name: "ListenFailed".into(),
                endpoint: Some(e.to_string()),
            },
        }
    }
}

impl From<PeerPoolEvent> for PeerPoolEventJson {
    fn from(item: PeerPoolEvent) -> Self {
        match item {
            PeerPoolEvent::NewPeer(id) => PeerPoolEventJson {
                name: "NewPeer".into(),
                peer_id: Some(id.to_string()),
            },
            PeerPoolEvent::RemovePeer(id) => PeerPoolEventJson {
                name: "RemovePeer".into(),
                peer_id: Some(id.to_string()),
            },
        }
    }
}

impl From<DiscoveryEvent> for DiscoveryEventJson {
    fn from(item: DiscoveryEvent) -> Self {
        match item {
            DiscoveryEvent::RefreshStarted => DiscoveryEventJson {
                name: "RefreshStarted".into(),
                endpoint: None,
            },
            DiscoveryEvent::LookupStarted(e) => DiscoveryEventJson {
                name: "LookupStarted".into(),
                endpoint: Some(e.to_string()),
            },
            DiscoveryEvent::LookupFailed(e) => DiscoveryEventJson {
                name: "LookupFailed".into(),
                endpoint: Some(e.to_string()),
            },
            DiscoveryEvent::LookupSucceeded(e, _) => DiscoveryEventJson {
                name: "LookupSucceeded".into(),
                endpoint: Some(e.to_string()),
            },
        }
    }
}
