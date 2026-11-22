mod shared;

use std::{net::TcpListener, sync::Arc, time::Duration};

use async_trait::async_trait;

use karyon_core::{
    async_runtime::Executor,
    async_util::{AsyncQueue, AsyncQueueRef, sleep, timeout},
    testing::run_test_with_executor,
};

use karyon_p2p::{
    Config, Error, Node, Version,
    endpoint::Endpoint,
    protocol::{PeerConn, Protocol, ProtocolID},
};

use shared::{create_node, fast_config, free_port};

const NODES: usize = 100;
const PROBE: &str = "PROBE";
const HELLO: &[u8] = b"hello";

/// Messages a `Probe` received.
type Inbox = AsyncQueueRef<Vec<u8>>;

/// Puts every message it receives in its inbox.
struct Probe {
    peer: PeerConn,
    inbox: Inbox,
}

#[async_trait]
impl Protocol for Probe {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        loop {
            match self.peer.recv().await {
                Ok(msg) => self.inbox.push(msg).await,
                Err(Error::PeerShutdown) => return Ok(()),
                Err(err) => return Err(err),
            }
        }
    }

    fn version() -> Result<Version, Error> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        PROBE.into()
    }
}

fn tcp(port: u16) -> Endpoint {
    format!("tcp://127.0.0.1:{port}").parse().unwrap()
}

#[cfg(feature = "quic")]
fn quic(port: u16) -> Endpoint {
    format!("quic://127.0.0.1:{port}").parse().unwrap()
}

/// `n` distinct free ports: all are held until the last one is taken.
fn free_ports(n: usize) -> Vec<u16> {
    let held: Vec<TcpListener> = (0..n)
        .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    held.iter()
        .map(|l| l.local_addr().unwrap().port())
        .collect()
}

/// A running node with a `Probe`.
async fn probe_node(config: Config, ex: Executor) -> (Arc<Node>, Inbox) {
    let node = create_node(config, ex);
    let inbox: Inbox = AsyncQueue::new(64);
    let probe_inbox = inbox.clone();
    node.attach_protocol(move |peer| Probe {
        peer,
        inbox: probe_inbox.clone(),
    })
    .await
    .unwrap();
    node.run().await.unwrap();
    (node, inbox)
}

/// Polls `node` until it has `n` peers, for at most `secs` seconds.
async fn wait_peers(node: &Node, n: usize, secs: u64) {
    let poll = async {
        while node.peers().await != n {
            sleep(Duration::from_millis(100)).await;
        }
    };
    if timeout(Duration::from_secs(secs), poll).await.is_err() {
        panic!("expected {n} peers, got {}", node.peers().await);
    }
}

/// 99 nodes dial one hub on `ep(port)` at once: all join, a broadcast
/// from the hub reaches all of them, and all leave on shutdown.
async fn hub_with_99_peers(ep: fn(u16) -> Endpoint, ex: Executor) {
    let port = free_port();
    let (hub, _) = probe_node(
        Config {
            listen_endpoints: vec![ep(port)],
            inbound_slots: NODES - 1,
            ..fast_config()
        },
        ex.clone(),
    )
    .await;

    let mut dialers = Vec::new();
    for _ in 1..NODES {
        let config = Config {
            dial_peers: vec![ep(port)],
            ..fast_config()
        };
        dialers.push(probe_node(config, ex.clone()).await);
    }

    wait_peers(&hub, NODES - 1, 60).await;
    assert_eq!(hub.inbound_peers().await.len(), NODES - 1);
    for (d, _) in &dialers {
        wait_peers(d, 1, 10).await;
    }

    hub.broadcast(&PROBE.into(), HELLO.to_vec()).await;
    for (i, (_, inbox)) in dialers.iter().enumerate() {
        let msg = timeout(Duration::from_secs(10), inbox.recv()).await;
        assert_eq!(msg.ok().as_deref(), Some(HELLO), "dialer {i}");
    }

    for (d, _) in &dialers {
        d.shutdown().await;
    }
    wait_peers(&hub, 0, 30).await;
    hub.shutdown().await;
}

#[test]
fn tcp_hub_with_99_peers() {
    run_test_with_executor(120, |ex| hub_with_99_peers(tcp, ex));
}

#[cfg(feature = "quic")]
#[test]
fn quic_hub_with_99_peers() {
    run_test_with_executor(120, |ex| hub_with_99_peers(quic, ex));
}

/// 99 nodes dial a hub with 50 inbound slots: the hub never goes over 50,
/// and the waiting nodes take the slots that free up.
#[test]
fn inbound_slots_full_under_load() {
    run_test_with_executor(120, |ex| async move {
        let slots = 50;
        let port = free_port();
        let (hub, _) = probe_node(
            Config {
                listen_endpoints: vec![tcp(port)],
                inbound_slots: slots,
                ..fast_config()
            },
            ex.clone(),
        )
        .await;

        // The waiting nodes stay in their handshake until a slot is free.
        let mut dialers = Vec::new();
        for _ in 1..NODES {
            let config = Config {
                dial_peers: vec![tcp(port)],
                handshake_timeout: 60,
                ..fast_config()
            };
            dialers.push(probe_node(config, ex.clone()).await.0);
        }

        wait_peers(&hub, slots, 30).await;
        for _ in 0..20 {
            assert!(hub.peers().await <= slots);
            sleep(Duration::from_millis(100)).await;
        }

        let mut left = 0;
        for d in &dialers {
            if left < 25 && d.peers().await == 1 {
                d.shutdown().await;
                left += 1;
            }
        }
        assert_eq!(left, 25);

        wait_peers(&hub, slots, 30).await;
        for _ in 0..20 {
            assert!(hub.peers().await <= slots);
            sleep(Duration::from_millis(100)).await;
        }

        for d in &dialers {
            d.shutdown().await;
        }
        hub.shutdown().await;
    });
}

/// 100 nodes in a ring, each dialing the next 4: every node gets 8 peers,
/// and a broadcast reaches all of them.
#[test]
fn mesh_of_100() {
    run_test_with_executor(120, |ex| async move {
        let ports = free_ports(NODES);
        let mut nodes = Vec::new();
        for (i, port) in ports.iter().enumerate() {
            let config = Config {
                listen_endpoints: vec![tcp(*port)],
                dial_peers: (1..=4).map(|k| tcp(ports[(i + k) % NODES])).collect(),
                // Some nodes dial before the next ones listen.
                max_connect_retries: 10,
                ..fast_config()
            };
            nodes.push(probe_node(config, ex.clone()).await);
        }

        for (node, _) in &nodes {
            wait_peers(node, 8, 60).await;
        }

        nodes[0].0.broadcast(&PROBE.into(), HELLO.to_vec()).await;
        let near = [1, 2, 3, 4, NODES - 4, NODES - 3, NODES - 2, NODES - 1];
        for i in near {
            let msg = timeout(Duration::from_secs(10), nodes[i].1.recv()).await;
            assert_eq!(msg.ok().as_deref(), Some(HELLO), "node {i}");
        }

        for (node, _) in &nodes {
            node.shutdown().await;
        }
    });
}
