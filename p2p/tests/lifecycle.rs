mod shared;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;

use karyon_core::{
    async_runtime::Executor,
    async_util::{AsyncQueue, AsyncQueueRef, sleep, timeout},
    testing::run_test_with_executor,
};
use karyon_eventemitter::EventListener;

use karyon_p2p::{
    Config, ConnDirection, Error, Node, PeerEvent, SessionInfoRef, Version,
    endpoint::Endpoint,
    monitor::{ConnectionEvent, MonitorTopic, PeerPoolEvent},
    protocol::{PeerConn, Protocol, ProtocolID},
};

use shared::{create_node, fast_config, free_port};

const PROBE: &str = "PROBE";
const HELLO: &[u8] = b"hello";

/// Messages a `Probe` received.
type Inbox = AsyncQueueRef<Vec<u8>>;

/// Puts every message it receives in its inbox. With `greet`, it first
/// broadcasts `HELLO` from inside `start`.
struct Probe {
    peer: PeerConn,
    inbox: Inbox,
    greet: bool,
}

#[async_trait]
impl Protocol for Probe {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        if self.greet {
            self.peer.broadcast(HELLO.to_vec()).await;
        }
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

const REPORT: &str = "REPORT";

/// Puts its session's `SessionInfo` in `infos`, then reads until the peer
/// shuts down.
struct Reporter {
    peer: PeerConn,
    infos: AsyncQueueRef<SessionInfoRef>,
}

#[async_trait]
impl Protocol for Reporter {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        self.infos.push(self.peer.info().clone()).await;
        loop {
            match self.peer.recv().await {
                Ok(_) => {}
                Err(Error::PeerShutdown) => return Ok(()),
                Err(err) => return Err(err),
            }
        }
    }

    fn version() -> Result<Version, Error> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        REPORT.into()
    }
}

fn tcp(port: u16) -> Endpoint {
    format!("tcp://127.0.0.1:{port}").parse().unwrap()
}

fn tls(port: u16) -> Endpoint {
    format!("tls://127.0.0.1:{port}").parse().unwrap()
}

/// A node listening on `listen` and dialing `dial`, with a `Probe`.
async fn probe_node(
    listen: Vec<Endpoint>,
    dial: Vec<Endpoint>,
    config: Config,
    greet: bool,
    ex: Executor,
) -> (Arc<Node>, Inbox) {
    let node = create_node(
        Config {
            listen_endpoints: listen,
            dial_peers: dial,
            ..config
        },
        ex,
    );
    let inbox: Inbox = AsyncQueue::new(64);
    let probe_inbox = inbox.clone();
    node.attach_protocol(move |peer| Probe {
        peer,
        inbox: probe_inbox.clone(),
        greet,
    })
    .await
    .unwrap();
    (node, inbox)
}

/// Next message in the inbox, or `None` after `secs` seconds.
async fn recv_within(inbox: &Inbox, secs: u64) -> Option<Vec<u8>> {
    timeout(Duration::from_secs(secs), inbox.recv()).await.ok()
}

/// Waits for the PeerPool event `name`, skipping the others.
async fn wait_pool(listener: &EventListener<MonitorTopic, PeerPoolEvent>, name: &str) {
    loop {
        if listener.recv().await.unwrap().event == name {
            return;
        }
    }
}

/// Waits for the connection event `name`, skipping the others.
async fn wait_conn(listener: &EventListener<MonitorTopic, ConnectionEvent>, name: &str) {
    loop {
        if listener.recv().await.unwrap().event == name {
            return;
        }
    }
}

/// Polls `node` until it has `n` peers, for at most `secs` seconds.
async fn wait_peers(node: &Node, n: usize, secs: u64) {
    let poll = async {
        while node.peers().await != n {
            sleep(Duration::from_millis(100)).await;
        }
    };
    timeout(Duration::from_secs(secs), poll)
        .await
        .unwrap_or_else(|_| panic!("expected {n} peers"));
}

/// A protocol broadcasts from inside `start`, and every peer gets it.
#[test]
fn protocol_broadcasts_from_start() {
    run_test_with_executor(30, |ex| async move {
        let port = free_port();
        let (node_a, _) =
            probe_node(vec![tcp(port)], vec![], fast_config(), true, ex.clone()).await;
        node_a.run().await.unwrap();

        let (node_b, inbox_b) =
            probe_node(vec![], vec![tcp(port)], fast_config(), false, ex.clone()).await;
        node_b.run().await.unwrap();
        let (node_c, inbox_c) =
            probe_node(vec![], vec![tcp(port)], fast_config(), false, ex.clone()).await;
        node_c.run().await.unwrap();

        assert_eq!(recv_within(&inbox_b, 10).await.as_deref(), Some(HELLO));
        assert_eq!(recv_within(&inbox_c, 10).await.as_deref(), Some(HELLO));

        node_c.shutdown().await;
        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

/// Two nodes dial each other at the same time and both keep the same
/// single connection.
#[test]
fn simultaneous_dial_keeps_one_connection() {
    run_test_with_executor(90, |ex| async move {
        for _ in 0..3 {
            let (port_a, port_b) = (free_port(), free_port());
            let (node_a, _) = probe_node(
                vec![tcp(port_a)],
                vec![tcp(port_b)],
                fast_config(),
                false,
                ex.clone(),
            )
            .await;
            let (node_b, _) = probe_node(
                vec![tcp(port_b)],
                vec![tcp(port_a)],
                fast_config(),
                false,
                ex.clone(),
            )
            .await;
            node_a.run().await.unwrap();
            node_b.run().await.unwrap();

            wait_peers(&node_a, 1, 15).await;
            wait_peers(&node_b, 1, 15).await;
            // Let the losing connection close.
            sleep(Duration::from_secs(2)).await;

            assert_eq!(node_a.peers().await, 1);
            assert_eq!(node_b.peers().await, 1);
            // The kept connection is outbound on one side, inbound on the other.
            assert_eq!(
                node_a.outbound_peers().await.len(),
                node_b.inbound_peers().await.len()
            );
            assert_eq!(
                node_a.inbound_peers().await.len(),
                node_b.outbound_peers().await.len()
            );

            node_b.shutdown().await;
            node_a.shutdown().await;
        }
    });
}

/// A second connection from the same peer is dropped, both nodes keep the
/// same one, and the inbound slot it took is freed for another peer.
#[test]
fn duplicate_connection_rejected_and_slot_freed() {
    run_test_with_executor(45, |ex| async move {
        let (tcp_port, tls_port) = (free_port(), free_port());
        let (node_a, _) = probe_node(
            vec![tcp(tcp_port), tls(tls_port)],
            vec![],
            Config {
                inbound_slots: 2,
                ..fast_config()
            },
            false,
            ex.clone(),
        )
        .await;
        node_a.run().await.unwrap();

        // B dials A on both endpoints: two connections, same direction.
        let (node_b, _) = probe_node(
            vec![],
            vec![tcp(tcp_port), tls(tls_port)],
            fast_config(),
            false,
            ex.clone(),
        )
        .await;
        node_b.run().await.unwrap();

        // Each node rejects or replaces the second connection, depending
        // on the order it admits them, so there may be no
        // `PeerAlreadyConnected` event. Both keep one and the same.
        wait_peers(&node_a, 1, 15).await;
        wait_peers(&node_b, 1, 15).await;

        // Only one of the two slots is in use, so C gets in.
        let (node_c, _) = probe_node(
            vec![],
            vec![tcp(tcp_port)],
            fast_config(),
            false,
            ex.clone(),
        )
        .await;
        node_c.run().await.unwrap();
        wait_peers(&node_a, 2, 15).await;

        node_c.shutdown().await;
        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

/// `inbound_slots` bounds the inbound peers, and a slot is freed once its
/// peer leaves.
#[test]
fn inbound_slots_limit_and_free() {
    run_test_with_executor(60, |ex| async move {
        let port = free_port();
        let (node_a, _) = probe_node(
            vec![tcp(port)],
            vec![],
            Config {
                inbound_slots: 1,
                ..fast_config()
            },
            false,
            ex.clone(),
        )
        .await;
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.unwrap();

        let (node_b, _) =
            probe_node(vec![], vec![tcp(port)], fast_config(), false, ex.clone()).await;
        node_b.run().await.unwrap();
        wait_pool(&pool_a, "NewPeer").await;

        // C waits long enough in its handshake to be accepted once B leaves.
        let (node_c, _) = probe_node(
            vec![],
            vec![tcp(port)],
            Config {
                handshake_timeout: 30,
                ..fast_config()
            },
            false,
            ex.clone(),
        )
        .await;
        node_c.run().await.unwrap();

        sleep(Duration::from_secs(3)).await;
        assert_eq!(node_a.peers().await, 1);
        assert!(node_a.peer_protocol_set(node_c.peer_id()).await.is_none());

        node_b.shutdown().await;
        wait_pool(&pool_a, "RemovePeer").await;
        timeout(Duration::from_secs(20), wait_pool(&pool_a, "NewPeer"))
            .await
            .expect("C joins once B's slot is free");
        assert!(node_a.peer_protocol_set(node_c.peer_id()).await.is_some());

        node_c.shutdown().await;
        node_a.shutdown().await;
    });
}

/// `register_peer_events` reports a peer joining and leaving.
#[test]
fn peer_events_added_removed() {
    run_test_with_executor(30, |ex| async move {
        let port = free_port();
        let (node_a, _) =
            probe_node(vec![tcp(port)], vec![], fast_config(), false, ex.clone()).await;
        let events = node_a.register_peer_events();
        node_a.run().await.unwrap();

        let (node_b, _) =
            probe_node(vec![], vec![tcp(port)], fast_config(), false, ex.clone()).await;
        node_b.run().await.unwrap();
        let b_id = node_b.peer_id().clone();

        match events.recv().await.unwrap() {
            PeerEvent::Added(pid) => assert_eq!(pid, b_id),
            ev => panic!("expected Added, got {ev:?}"),
        }

        node_b.shutdown().await;
        match events.recv().await.unwrap() {
            PeerEvent::Removed(pid) => assert_eq!(pid, b_id),
            ev => panic!("expected Removed, got {ev:?}"),
        }

        node_a.shutdown().await;
    });
}

/// A peer on another network version fails the handshake, reported to
/// the peer events and the monitor.
#[test]
fn peer_events_handshake_failed() {
    run_test_with_executor(30, |ex| async move {
        let port = free_port();
        let (node_a, _) =
            probe_node(vec![tcp(port)], vec![], fast_config(), false, ex.clone()).await;
        let events = node_a.register_peer_events();
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.unwrap();

        let (node_b, _) = probe_node(
            vec![],
            vec![tcp(port)],
            Config {
                version: "99.0.0".parse().unwrap(),
                ..fast_config()
            },
            false,
            ex.clone(),
        )
        .await;
        node_b.run().await.unwrap();

        match events.recv().await.unwrap() {
            PeerEvent::HandshakeFailed(_) => {}
            ev => panic!("expected HandshakeFailed, got {ev:?}"),
        }
        wait_pool(&pool_a, "HandshakeFailed").await;
        assert_eq!(node_a.peers().await, 0);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

/// Shutting a node down disconnects its peers.
#[test]
fn shutdown_disconnects_peers() {
    run_test_with_executor(30, |ex| async move {
        let port = free_port();
        let (node_a, _) =
            probe_node(vec![tcp(port)], vec![], fast_config(), false, ex.clone()).await;
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        let conn_a = node_a.monitor().register::<ConnectionEvent>();
        node_a.run().await.unwrap();

        let (node_b, inbox_b) =
            probe_node(vec![], vec![tcp(port)], fast_config(), false, ex.clone()).await;
        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        node_b.run().await.unwrap();
        wait_pool(&pool_a, "NewPeer").await;
        wait_pool(&pool_b, "NewPeer").await;

        node_a.shutdown().await;
        // Nothing is sent once the node is shut down.
        node_a.broadcast(&PROBE.into(), HELLO.to_vec()).await;
        assert!(recv_within(&inbox_b, 1).await.is_none());
        timeout(Duration::from_secs(5), wait_conn(&conn_a, "Disconnected"))
            .await
            .expect("A reports the disconnect");
        timeout(Duration::from_secs(15), wait_pool(&pool_b, "RemovePeer"))
            .await
            .expect("B sees A leave");
        assert_eq!(node_b.peers().await, 0);

        node_b.shutdown().await;
    });
}

/// `Node::broadcast` reaches every connected peer.
#[test]
fn node_broadcast_reaches_all_peers() {
    run_test_with_executor(30, |ex| async move {
        let port = free_port();
        let (node_a, _) =
            probe_node(vec![tcp(port)], vec![], fast_config(), false, ex.clone()).await;
        node_a.run().await.unwrap();

        let (node_b, inbox_b) =
            probe_node(vec![], vec![tcp(port)], fast_config(), false, ex.clone()).await;
        node_b.run().await.unwrap();
        let (node_c, inbox_c) =
            probe_node(vec![], vec![tcp(port)], fast_config(), false, ex.clone()).await;
        node_c.run().await.unwrap();
        wait_peers(&node_a, 2, 15).await;

        node_a.broadcast(&PROBE.into(), HELLO.to_vec()).await;
        assert_eq!(recv_within(&inbox_b, 10).await.as_deref(), Some(HELLO));
        assert_eq!(recv_within(&inbox_c, 10).await.as_deref(), Some(HELLO));

        node_c.shutdown().await;
        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

/// `PeerConn::info` describes the session on both sides.
#[test]
fn session_info_matches_connection() {
    run_test_with_executor(30, |ex| async move {
        let port = free_port();
        let node_a = create_node(
            Config {
                listen_endpoints: vec![tcp(port)],
                ..fast_config()
            },
            ex.clone(),
        );
        let infos_a: AsyncQueueRef<SessionInfoRef> = AsyncQueue::new(4);
        let infos = infos_a.clone();
        node_a
            .attach_protocol(move |peer| Reporter {
                peer,
                infos: infos.clone(),
            })
            .await
            .unwrap();
        node_a.run().await.unwrap();

        let node_b = create_node(
            Config {
                dial_peers: vec![tcp(port)],
                ..fast_config()
            },
            ex.clone(),
        );
        let infos_b: AsyncQueueRef<SessionInfoRef> = AsyncQueue::new(4);
        let infos = infos_b.clone();
        node_b
            .attach_protocol(move |peer| Reporter {
                peer,
                infos: infos.clone(),
            })
            .await
            .unwrap();
        node_b.run().await.unwrap();

        let info_a = timeout(Duration::from_secs(10), infos_a.recv())
            .await
            .expect("A's session info");
        let info_b = timeout(Duration::from_secs(10), infos_b.recv())
            .await
            .expect("B's session info");

        assert_eq!(&info_a.id, node_b.peer_id());
        assert!(matches!(info_a.direction, ConnDirection::Inbound));
        let (report, ping): (ProtocolID, ProtocolID) = (REPORT.into(), "PING".into());
        assert!(info_a.negotiated.contains(&report));
        assert!(info_a.negotiated.contains(&ping));

        assert_eq!(&info_b.id, node_a.peer_id());
        assert!(matches!(info_b.direction, ConnDirection::Outbound));
        assert_eq!(info_b.endpoint, tcp(port));
        assert_eq!(info_b.negotiated, info_a.negotiated);

        let set = node_a.peer_protocol_set(node_b.peer_id()).await;
        assert_eq!(set.as_ref(), Some(&info_a.negotiated));

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}
