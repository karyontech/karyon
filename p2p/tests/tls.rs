mod shared;

use std::{net::TcpListener, time::Duration};

use karyon_core::{async_util::timeout, testing::run_test_with_executor};

use karyon_p2p::{Config, monitor::PeerPoolEvent};

use shared::{create_node, fast_config, free_port};

#[test]
fn two_nodes_direct_connect() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();

        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tls://127.0.0.1:{listen_port}").parse().unwrap()],
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
