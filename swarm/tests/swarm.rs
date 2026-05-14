mod shared;

use std::sync::Arc;

use async_trait::async_trait;

use karyon_core::testing::run_test_with_executor;

use karyon_p2p::{
    endpoint::Endpoint,
    monitor::PeerPoolEvent,
    protocol::{PeerConn, Protocol, ProtocolID},
    Config, Error, Version,
};

use karyon_swarm::{compute_swarm_key, Swarm};

use shared::{create_node, fast_config, free_port};

// -- Test protocols ----------------------------------------------------------

struct ChatProtocol {
    peer: PeerConn,
}

impl ChatProtocol {
    fn new(peer: PeerConn) -> Arc<dyn Protocol> {
        Arc::new(Self { peer })
    }
}

#[async_trait]
impl Protocol for ChatProtocol {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        loop {
            match self.peer.recv().await {
                Ok(_) => {}
                Err(Error::PeerShutdown) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn version() -> Result<Version, Error> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "Chat".into()
    }
}

struct FileProtocol {
    peer: PeerConn,
}

impl FileProtocol {
    fn new(peer: PeerConn) -> Arc<dyn Protocol> {
        Arc::new(Self { peer })
    }
}

#[async_trait]
impl Protocol for FileProtocol {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        loop {
            match self.peer.recv().await {
                Ok(_) => {}
                Err(Error::PeerShutdown) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn version() -> Result<Version, Error> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "Fs".into()
    }
}

// -- Tests -------------------------------------------------------------------

/// Two nodes with different protocol sets can connect. Before the swarm
/// changes this would have been rejected by the handshake.
#[test]
fn heterogeneous_protocols_connect() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();

        // Node A: ChatProto + FileProto
        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{discovery_port}").parse().unwrap(),
                    format!("udp://127.0.0.1:{discovery_port}").parse().unwrap(),
                ],
                ..fast_config()
            },
            ex.clone(),
        );

        node_a
            .attach_protocol::<ChatProtocol>(|peer| Ok(ChatProtocol::new(peer)))
            .await
            .unwrap();
        node_a
            .attach_protocol::<FileProtocol>(|peer| Ok(FileProtocol::new(peer)))
            .await
            .unwrap();

        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.unwrap();

        // Node B: only ChatProto (no FileProto)
        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );

        node_b
            .attach_protocol::<ChatProtocol>(|peer| Ok(ChatProtocol::new(peer)))
            .await
            .unwrap();

        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        node_b.run().await.unwrap();

        // Both should connect (intersection = ChatProto + Ping).
        let ev_a = pool_a.recv().await.unwrap();
        assert_eq!(ev_a.event, "NewPeer");

        let ev_b = pool_b.recv().await.unwrap();
        assert_eq!(ev_b.event, "NewPeer");

        assert_eq!(node_a.peers().await, 1);
        assert_eq!(node_b.peers().await, 1);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

/// Swarm tracks peers per-protocol. Node A (Chat+File) and Node B (Chat only)
/// connect. The Chat swarm should see the peer, the File swarm should not.
#[test]
fn swarm_membership() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();

        // Node A: Chat + File, wrapped in Swarm
        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{discovery_port}").parse().unwrap(),
                    format!("udp://127.0.0.1:{discovery_port}").parse().unwrap(),
                ],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        let swarm_a = Swarm::new(node_a, ex.clone());

        let chat_key = swarm_a
            .join::<ChatProtocol>(|peer| Ok(ChatProtocol::new(peer)))
            .await
            .unwrap();
        let file_key = swarm_a
            .join::<FileProtocol>(|peer| Ok(FileProtocol::new(peer)))
            .await
            .unwrap();

        swarm_a.run().await.unwrap();

        // Node B: Chat only
        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        let swarm_b = Swarm::new(node_b, ex.clone());

        swarm_b
            .join::<ChatProtocol>(|peer| Ok(ChatProtocol::new(peer)))
            .await
            .unwrap();

        swarm_b.run().await.unwrap();

        // Wait for connection.
        loop {
            let ev = pool_a.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }
        loop {
            let ev = pool_b.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }
        // Let the swarm monitor task process the PeerEvent.
        karyon_core::async_util::sleep(std::time::Duration::from_millis(100)).await;

        // Chat swarm on A should see node B.
        assert_eq!(swarm_a.peers_len(&chat_key).await, 1);
        // File swarm on A should NOT see node B (B doesn't have FileProto).
        assert_eq!(swarm_a.peers_len(&file_key).await, 0);

        swarm_b.shutdown().await;
        swarm_a.shutdown().await;
    });
}

/// Swarm key is deterministic.
#[test]
fn swarm_key_deterministic() {
    let a = compute_swarm_key("ChatProto", "room1");
    let b = compute_swarm_key("ChatProto", "room1");
    assert_eq!(a, b);

    let c = compute_swarm_key("ChatProto", "room2");
    assert_ne!(a, c);
}

/// Kademlia routing-table entries carry a Bloom of the peer's registered
/// protocols. The connector skips entries whose bloom does not cover the
/// local protocol set, so a node with only `ChatProto` should never dial a
/// node that only speaks `FileProto`, even if Kademlia surfaces them in the
/// same routing table.
#[test]
fn bloom_filters_kademlia_dial() {
    run_test_with_executor(45, |ex| async move {
        // Bootstrap node: only the core Ping protocol.
        let bootstrap_listen = free_port();
        let bootstrap_disc = free_port();
        let bootstrap = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{bootstrap_listen}")
                    .parse()
                    .unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{bootstrap_disc}").parse().unwrap(),
                    format!("udp://127.0.0.1:{bootstrap_disc}").parse().unwrap(),
                ],
                seeding_interval: 1,
                ..fast_config()
            },
            ex.clone(),
        );
        bootstrap.run().await.unwrap();

        let bootstrap_ep: Endpoint = format!("tcp://127.0.0.1:{bootstrap_disc}").parse().unwrap();

        // Node A speaks ChatProto.
        let a_listen = free_port();
        let a_disc = free_port();
        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{a_listen}").parse().unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{a_disc}").parse().unwrap(),
                    format!("udp://127.0.0.1:{a_disc}").parse().unwrap(),
                ],
                bootstrap_peers: vec![bootstrap_ep.clone()],
                seeding_interval: 1,
                ..fast_config()
            },
            ex.clone(),
        );
        node_a
            .attach_protocol::<ChatProtocol>(|peer| Ok(ChatProtocol::new(peer)))
            .await
            .unwrap();
        node_a.run().await.unwrap();

        // Node C speaks FileProto.
        let c_listen = free_port();
        let c_disc = free_port();
        let node_c = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{c_listen}").parse().unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{c_disc}").parse().unwrap(),
                    format!("udp://127.0.0.1:{c_disc}").parse().unwrap(),
                ],
                bootstrap_peers: vec![bootstrap_ep],
                seeding_interval: 1,
                ..fast_config()
            },
            ex.clone(),
        );
        node_c
            .attach_protocol::<FileProtocol>(|peer| Ok(FileProtocol::new(peer)))
            .await
            .unwrap();
        node_c.run().await.unwrap();

        // Give Kademlia time to bootstrap and propagate entries between
        // A and C through the bootstrap node, then run a few seeding
        // cycles. With the bloom filter active, neither A nor C should
        // ever pick the other from the routing table.
        karyon_core::async_util::sleep(std::time::Duration::from_secs(8)).await;

        let a_outbound = node_a.outbound_peers().await;
        let c_peer_id = node_c.peer_id().clone();
        assert!(
            !a_outbound.contains_key(&c_peer_id),
            "node_a dialed node_c despite no shared application protocols"
        );

        let c_outbound = node_c.outbound_peers().await;
        let a_peer_id = node_a.peer_id().clone();
        assert!(
            !c_outbound.contains_key(&a_peer_id),
            "node_c dialed node_a despite no shared application protocols"
        );

        node_c.shutdown().await;
        node_a.shutdown().await;
        bootstrap.shutdown().await;
    });
}

/// Peers removed from the network are cleaned up from swarms.
#[test]
fn swarm_peer_removal() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();

        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{discovery_port}").parse().unwrap(),
                    format!("udp://127.0.0.1:{discovery_port}").parse().unwrap(),
                ],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        let swarm_a = Swarm::new(node_a, ex.clone());

        let chat_key = swarm_a
            .join::<ChatProtocol>(|peer| Ok(ChatProtocol::new(peer)))
            .await
            .unwrap();

        swarm_a.run().await.unwrap();

        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );

        let swarm_b = Swarm::new(node_b, ex.clone());
        swarm_b
            .join::<ChatProtocol>(|peer| Ok(ChatProtocol::new(peer)))
            .await
            .unwrap();
        swarm_b.run().await.unwrap();

        // Wait for peer added.
        loop {
            let ev = pool_a.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }
        // Let the swarm monitor task process the PeerEvent.
        karyon_core::async_util::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(swarm_a.peers_len(&chat_key).await, 1);

        // Shut down node B - should trigger removal.
        swarm_b.shutdown().await;

        loop {
            let ev = pool_a.recv().await.unwrap();
            if ev.event == "RemovePeer" {
                break;
            }
        }

        // Give the swarm monitor task a moment to process the event.
        karyon_core::async_util::sleep(std::time::Duration::from_millis(100)).await;

        assert_eq!(swarm_a.peers_len(&chat_key).await, 0);

        swarm_a.shutdown().await;
    });
}
