mod shared;

use std::{net::TcpListener, time::Duration};

use karyon_core::{async_util::timeout, testing::run_test_with_executor};

use karyon_p2p::{Config, endpoint::Endpoint, monitor::PeerPoolEvent};

use shared::{create_node, fast_config, free_port};

#[test]
fn two_nodes_direct_connect() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();

        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tls://127.0.0.1:{listen_port}").parse().unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{discovery_port}").parse().unwrap(),
                    format!("udp://127.0.0.1:{discovery_port}").parse().unwrap(),
                ],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        let node_b = create_node(
            Config {
                dial_peers: vec![format!("tls://127.0.0.1:{listen_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        node_b.run().await.expect("node_b run");

        let ev_a = pool_a.recv().await.unwrap();
        assert_eq!(ev_a.event, "NewPeer");

        let ev_b = pool_b.recv().await.unwrap();
        assert_eq!(ev_b.event, "NewPeer");

        assert_eq!(node_a.peers().await, 1);
        assert_eq!(node_b.peers().await, 1);
        assert_eq!(node_a.inbound_peers().await.len(), 1);
        assert_eq!(node_b.outbound_peers().await.len(), 1);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

#[test]
fn discovery_network() {
    run_test_with_executor(60, |ex| async move {
        let n1_listen_port = free_port();
        let n1_discovery_port = free_port();

        let node1 = create_node(
            Config {
                listen_endpoints: vec![
                    format!("tls://127.0.0.1:{n1_listen_port}").parse().unwrap(),
                ],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{n1_discovery_port}")
                        .parse()
                        .unwrap(),
                    format!("udp://127.0.0.1:{n1_discovery_port}")
                        .parse()
                        .unwrap(),
                ],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool1 = node1.monitor().register::<PeerPoolEvent>();
        node1.run().await.expect("node1 run");

        let bootstrap_ep: Endpoint = format!("tcp://127.0.0.1:{n1_discovery_port}")
            .parse()
            .unwrap();

        let n2_listen_port = free_port();
        let n2_discovery_port = free_port();
        let node2 = create_node(
            Config {
                listen_endpoints: vec![
                    format!("tls://127.0.0.1:{n2_listen_port}").parse().unwrap(),
                ],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{n2_discovery_port}")
                        .parse()
                        .unwrap(),
                    format!("udp://127.0.0.1:{n2_discovery_port}")
                        .parse()
                        .unwrap(),
                ],
                bootstrap_peers: vec![bootstrap_ep.clone()],
                ..fast_config()
            },
            ex.clone(),
        );

        let node3 = create_node(
            Config {
                bootstrap_peers: vec![bootstrap_ep],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool3 = node3.monitor().register::<PeerPoolEvent>();

        node2.run().await.expect("node2 run");
        node3.run().await.expect("node3 run");

        loop {
            let ev = pool3.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }
        assert!(node3.peers().await >= 1);

        loop {
            let ev = pool1.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }
        assert!(node1.peers().await >= 1);

        node3.shutdown().await;
        node2.shutdown().await;
        node1.shutdown().await;
    });
}

/// An endpoint that accepts the TCP connection and never answers the TLS
/// handshake must not stop the dial to the next peer.
#[test]
fn silent_tls_endpoint_does_not_block_next_dial() {
    run_test_with_executor(30, |ex| async move {
        // Never accepts: the kernel completes the TCP handshake, and the
        // TLS ClientHello gets no answer.
        let silent = TcpListener::bind("127.0.0.1:0").expect("bind silent listener");
        let silent_port = silent.local_addr().unwrap().port();

        let listen_port = free_port();
        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tls://127.0.0.1:{listen_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );
        node_a.run().await.expect("node_a run");

        let node_b = create_node(
            Config {
                dial_peers: vec![
                    format!("tls://127.0.0.1:{silent_port}").parse().unwrap(),
                    format!("tls://127.0.0.1:{listen_port}").parse().unwrap(),
                ],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        node_b.run().await.expect("node_b run");

        let new_peer = timeout(Duration::from_secs(4), async {
            while pool_b.recv().await.unwrap().event != "NewPeer" {}
        })
        .await;
        assert!(
            new_peer.is_ok(),
            "silent TLS endpoint blocked the next dial"
        );
        assert_eq!(node_b.peers().await, 1);

        drop(silent);
        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}
