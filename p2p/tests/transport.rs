mod shared;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;

use karyon_core::testing::run_test_with_executor;
use karyon_net::tcp::{self, TcpListener};

use karyon_p2p::{
    ConfigBuilder, Result,
    endpoint::Endpoint,
    monitor::PeerPoolEvent,
    transport::{ByteStream, Transport, TransportListener, TransportRef},
};

use shared::{create_node, fast_config, free_port};

/// TCP through the `Transport` trait, counting its dials and accepts.
struct CountingTcp {
    dials: Arc<AtomicUsize>,
    accepts: Arc<AtomicUsize>,
}

#[async_trait]
impl Transport for CountingTcp {
    fn supports(&self, endpoint: &Endpoint) -> bool {
        matches!(endpoint, Endpoint::Tcp(..))
    }

    async fn dial(&self, endpoint: &Endpoint) -> Result<Box<dyn ByteStream>> {
        self.dials.fetch_add(1, Ordering::SeqCst);
        Ok(tcp::connect(endpoint, Default::default()).await?)
    }

    async fn listen(&self, endpoint: &Endpoint) -> Result<Box<dyn TransportListener>> {
        let inner = TcpListener::bind(endpoint, Default::default()).await?;
        Ok(Box::new(CountingListener {
            inner,
            accepts: self.accepts.clone(),
        }))
    }
}

struct CountingListener {
    inner: TcpListener,
    accepts: Arc<AtomicUsize>,
}

#[async_trait]
impl TransportListener for CountingListener {
    async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        let stream = self.inner.accept().await?;
        self.accepts.fetch_add(1, Ordering::SeqCst);
        Ok(stream)
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(self.inner.local_endpoint()?)
    }
}

/// Two nodes connect through a user transport instead of the built-in
/// TCP.
#[test]
fn nodes_connect_through_user_transport() {
    run_test_with_executor(30, |ex| async move {
        let port = free_port();
        let endpoint: Endpoint = format!("tcp://127.0.0.1:{port}").parse().unwrap();
        let dials = Arc::new(AtomicUsize::new(0));
        let accepts = Arc::new(AtomicUsize::new(0));
        let transport = || {
            Arc::new(CountingTcp {
                dials: dials.clone(),
                accepts: accepts.clone(),
            }) as TransportRef
        };
        let base = || {
            let fast = fast_config();
            ConfigBuilder::new()
                .with_monitor(fast.enable_monitor)
                .with_ping_interval(fast.ping_interval)
                .with_ping_timeout(fast.ping_timeout)
                .with_handshake_timeout(fast.handshake_timeout)
                .with_transport(transport())
        };

        let node_a = create_node(
            base().with_listen_endpoints(vec![endpoint.clone()]).build(),
            ex.clone(),
        );
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.unwrap();

        let node_b = create_node(base().with_dial_peers(vec![endpoint]).build(), ex.clone());
        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        node_b.run().await.unwrap();

        assert_eq!(pool_a.recv().await.unwrap().event, "NewPeer");
        assert_eq!(pool_b.recv().await.unwrap().event, "NewPeer");
        assert_eq!(dials.load(Ordering::SeqCst), 1);
        assert_eq!(accepts.load(Ordering::SeqCst), 1);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}
