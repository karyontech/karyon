mod shared;

use std::sync::Arc;

use clap::Parser;
use log::error;
use ringbuffer::{AllocRingBuffer, RingBuffer};
use serde::{Deserialize, Serialize};
use smol::{channel, lock::Mutex, Executor};

use karyon_core::async_util::{CondWait, TaskGroup, TaskResult};
use karyon_jsonrpc::{rpc_impl, rpc_pubsub_impl, ArcChannel, Server, Subscription, SubscriptionID};
use karyon_p2p::{
    endpoint::{Endpoint, Port},
    keypair::{KeyPair, KeyPairType},
    monitor::{ConnEvent, DiscoveryEvent, PeerPoolEvent},
    ArcBackend, Backend, Config, Error, Result,
};

use shared::run_executor;

const EVENT_BUFFER_SIZE: usize = 30;

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
    conn_event_buffer: Arc<Mutex<AllocRingBuffer<ConnEvent>>>,
    pp_event_buffer: Arc<Mutex<AllocRingBuffer<PeerPoolEvent>>>,
    discv_event_buffer: Arc<Mutex<AllocRingBuffer<DiscoveryEvent>>>,
    conn_event_condvar: Arc<CondWait>,
    pp_event_condvar: Arc<CondWait>,
    discv_event_condvar: Arc<CondWait>,
    task_group: TaskGroup,
}

impl MonitorRPC {
    fn new(backend: ArcBackend, ex: Arc<Executor<'static>>) -> Arc<Self> {
        Arc::new(MonitorRPC {
            backend,
            conn_event_buffer: Arc::new(Mutex::new(AllocRingBuffer::new(EVENT_BUFFER_SIZE))),
            pp_event_buffer: Arc::new(Mutex::new(AllocRingBuffer::new(EVENT_BUFFER_SIZE))),
            discv_event_buffer: Arc::new(Mutex::new(AllocRingBuffer::new(EVENT_BUFFER_SIZE))),
            conn_event_condvar: Arc::new(CondWait::new()),
            pp_event_condvar: Arc::new(CondWait::new()),
            discv_event_condvar: Arc::new(CondWait::new()),
            task_group: TaskGroup::with_executor(ex.into()),
        })
    }

    async fn run(&self) -> Result<()> {
        let conn_events = self.backend.monitor().conn_events().await;
        let peer_pool_events = self.backend.monitor().peer_pool_events().await;
        let discovery_events = self.backend.monitor().discovery_events().await;

        let conn_event_buffer = self.conn_event_buffer.clone();
        let pp_event_buffer = self.pp_event_buffer.clone();
        let discv_event_buffer = self.discv_event_buffer.clone();

        let conn_event_condvar = self.conn_event_condvar.clone();
        let pp_event_condvar = self.pp_event_condvar.clone();
        let discv_event_condvar = self.discv_event_condvar.clone();

        let on_failuer = |res: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = res {
                error!("Event receive loop: {err}")
            }
        };

        self.task_group.spawn(
            async move {
                loop {
                    let event = conn_events.recv().await?;
                    conn_event_buffer.lock().await.push(event);
                    conn_event_condvar.broadcast().await;
                }
            },
            on_failuer,
        );

        self.task_group.spawn(
            async move {
                loop {
                    let event = peer_pool_events.recv().await?;
                    pp_event_buffer.lock().await.push(event);
                    pp_event_condvar.broadcast().await;
                }
            },
            on_failuer,
        );

        self.task_group.spawn(
            async move {
                loop {
                    let event = discovery_events.recv().await?;
                    discv_event_buffer.lock().await.push(event);
                    discv_event_condvar.broadcast().await;
                }
            },
            on_failuer,
        );

        Ok(())
    }

    async fn shutdown(&self) {
        self.task_group.cancel().await;
    }
}

#[rpc_impl]
impl MonitorRPC {
    async fn peer_id(
        &self,
        _params: serde_json::Value,
    ) -> karyon_jsonrpc::Result<serde_json::Value> {
        Ok(serde_json::json!(self.backend.peer_id().to_string()))
    }

    async fn inbound_connection(
        &self,
        _params: serde_json::Value,
    ) -> karyon_jsonrpc::Result<serde_json::Value> {
        Ok(serde_json::json!(self.backend.inbound_slots()))
    }

    async fn outbound_connection(
        &self,
        _params: serde_json::Value,
    ) -> karyon_jsonrpc::Result<serde_json::Value> {
        Ok(serde_json::json!(self.backend.outbound_slots()))
    }
}

#[rpc_pubsub_impl]
impl MonitorRPC {
    async fn conn_subscribe(
        &self,
        chan: ArcChannel,
        method: String,
        _params: serde_json::Value,
    ) -> karyon_jsonrpc::Result<serde_json::Value> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id.clone();

        let cond_wait = self.conn_event_condvar.clone();
        let buffer = self.conn_event_buffer.clone();
        self.task_group
            .spawn(notify(sub, cond_wait, buffer), notify_failed);

        Ok(serde_json::json!(sub_id))
    }

    async fn peer_pool_subscribe(
        &self,
        chan: ArcChannel,
        method: String,
        _params: serde_json::Value,
    ) -> karyon_jsonrpc::Result<serde_json::Value> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id.clone();

        let cond_wait = self.pp_event_condvar.clone();
        let buffer = self.pp_event_buffer.clone();
        self.task_group
            .spawn(notify(sub, cond_wait, buffer), notify_failed);

        Ok(serde_json::json!(sub_id))
    }

    async fn discovery_subscribe(
        &self,
        chan: ArcChannel,
        method: String,
        _params: serde_json::Value,
    ) -> karyon_jsonrpc::Result<serde_json::Value> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id.clone();

        let cond_wait = self.discv_event_condvar.clone();
        let buffer = self.discv_event_buffer.clone();
        self.task_group
            .spawn(notify(sub, cond_wait, buffer), notify_failed);

        Ok(serde_json::json!(sub_id))
    }

    async fn conn_unsubscribe(
        &self,
        chan: ArcChannel,
        _method: String,
        params: serde_json::Value,
    ) -> karyon_jsonrpc::Result<serde_json::Value> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        chan.remove_subscription(&sub_id).await;
        Ok(serde_json::json!(true))
    }

    async fn peer_pool_unsubscribe(
        &self,
        chan: ArcChannel,
        _method: String,
        params: serde_json::Value,
    ) -> karyon_jsonrpc::Result<serde_json::Value> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        chan.remove_subscription(&sub_id).await;
        Ok(serde_json::json!(true))
    }

    async fn discovery_unsubscribe(
        &self,
        chan: ArcChannel,
        _method: String,
        params: serde_json::Value,
    ) -> karyon_jsonrpc::Result<serde_json::Value> {
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
            let service = MonitorRPC::new(backend.clone(), exc.clone());

            // Create rpc server
            let server = Server::builder(cli.rpc_endpoint)
                .expect("Create server builder")
                .service(service.clone())
                .pubsub_service(service.clone())
                .build_with_executor(exc.clone().into())
                .await
                .expect("Build rpc server");

            // Run the RPC server
            server.start().await;

            // Run the RPC Service
            service.run().await.expect("Run monitor rpc service");

            // Run the backend
            backend.run().await.expect("Run p2p backend");

            // Wait for ctrlc signal
            ctrlc_r.recv().await.expect("Wait for ctrlc signal");

            // Shutdown the backend
            backend.shutdown().await;

            // Shutdown the RPC server
            server.shutdown().await;

            // Shutdown the RPC service
            service.shutdown().await;
        },
        ex,
    );
}

async fn notify<T: Serialize + Deserialize<'static> + Clone>(
    sub: Subscription,
    cond_wait: Arc<CondWait>,
    buffer: Arc<Mutex<AllocRingBuffer<T>>>,
) -> Result<()> {
    for event in buffer.lock().await.iter() {
        if let Err(err) = sub.notify(serde_json::json!(event)).await {
            return Err(Error::Other(format!("failed to notify: {err}")));
        }
    }
    loop {
        cond_wait.wait().await;
        cond_wait.reset().await;
        if let Some(event) = buffer.lock().await.back().cloned() {
            if let Err(err) = sub.notify(serde_json::json!(event)).await {
                return Err(Error::Other(format!("failed to notify: {err}")));
            }
        }
    }
}

async fn notify_failed(result: TaskResult<Result<()>>) {
    if let TaskResult::Completed(Err(err)) = result {
        error!("Error: {err}");
    }
}
