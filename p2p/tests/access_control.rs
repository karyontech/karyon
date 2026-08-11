mod shared;

use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use karyon_core::{async_util::sleep, testing::run_test_with_executor};

use karyon_p2p::{
    monitor::PeerPoolEvent, AccessControl, Action, Config, ConnDirection, PeerID, Subject,
};

use shared::{create_node, fast_config, free_port};

/// Policy based on peer ids: an allow list, a deny list, or both.
///
/// Deny takes precedence. An empty allow list accepts any peer that is
/// not denied; a non-empty one denies any peer that is not allowed.
#[derive(Default)]
struct PeerIdList {
    allowed: HashSet<PeerID>,
    denied: HashSet<PeerID>,
}

impl PeerIdList {
    fn new() -> Self {
        Self::default()
    }

    fn allow(mut self, id: PeerID) -> Self {
        self.allowed.insert(id);
        self
    }

    fn deny(mut self, id: PeerID) -> Self {
        self.denied.insert(id);
        self
    }
}

impl AccessControl for PeerIdList {
    fn allow(&self, subject: &Subject, _action: Action) -> bool {
        let peer = match subject {
            // An id list cannot evaluate a bare address.
            Subject::Endpoint(_) => return true,
            Subject::Peer(p) => p,
        };

        if self.denied.contains(peer.peer_id) {
            return false;
        }
        self.allowed.is_empty() || self.allowed.contains(peer.peer_id)
    }
}

/// Denies every inbound endpoint and counts how many times it was asked.
struct DenyInbound {
    calls: Arc<AtomicUsize>,
}

impl AccessControl for DenyInbound {
    fn allow(&self, subject: &Subject, action: Action) -> bool {
        match (subject, action) {
            (Subject::Endpoint(_), Action::Connect(ConnDirection::Inbound)) => {
                self.calls.fetch_add(1, Ordering::SeqCst);
                false
            }
            _ => true,
        }
    }
}

#[test]
fn denied_peer_never_joins_pool() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();

        // Built first so its id can go on node_a's deny list.
        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tls://127.0.0.1:{listen_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );

        let banned = node_b.peer_id().clone();

        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tls://127.0.0.1:{listen_port}").parse().unwrap()],
                access_control: Arc::new(PeerIdList::new().deny(banned)),
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");
        node_b.run().await.expect("node_b run");

        // The handshake succeeds, then the admission gate drops it.
        loop {
            let ev = pool_a.recv().await.unwrap();
            assert_ne!(ev.event, "NewPeer", "denied peer joined the pool");
            if ev.event == "HandshakeFailed" {
                break;
            }
        }

        assert_eq!(node_a.peers().await, 0);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

#[test]
fn allowed_peer_joins_pool() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();

        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tls://127.0.0.1:{listen_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );

        let allowed = node_b.peer_id().clone();

        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tls://127.0.0.1:{listen_port}").parse().unwrap()],
                access_control: Arc::new(PeerIdList::new().allow(allowed)),
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");
        node_b.run().await.expect("node_b run");

        let ev = pool_a.recv().await.unwrap();
        assert_eq!(ev.event, "NewPeer");
        assert_eq!(node_a.peers().await, 1);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

#[test]
fn denied_endpoint_rejected_before_handshake() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();
        let calls = Arc::new(AtomicUsize::new(0));

        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                access_control: Arc::new(DenyInbound {
                    calls: calls.clone(),
                }),
                ..fast_config()
            },
            ex.clone(),
        );

        node_a.run().await.expect("node_a run");

        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );
        node_b.run().await.expect("node_b run");

        // Wait for the gate to see at least one inbound connection.
        while calls.load(Ordering::SeqCst) == 0 {
            sleep(Duration::from_millis(50)).await;
        }

        // Dropped on the accept loop, so no peer is ever queued.
        assert_eq!(node_a.peers().await, 0);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}
