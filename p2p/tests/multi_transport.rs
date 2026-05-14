/// Tests that verify the p2p layer correctly selects and uses different
/// transports when nodes are configured with mixed endpoint types.
mod shared;

use karyon_core::testing::run_test_with_executor;

use karyon_p2p::{monitor::PeerPoolEvent, Config};

use shared::{create_node, fast_config, free_port};

/// Test: Node A listens on TCP, Node B connects. Then Node C listens on TLS,
/// Node D connects. Validates both transport types work within the same test.
#[test]
fn tcp_and_tls_coexist() {
    run_test_with_executor(30, |ex| async move {
        // TCP pair
        let tcp_port = free_port();
        let tcp_disc = free_port();

        let tcp_listener = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{tcp_port}").parse().unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{tcp_disc}").parse().unwrap(),
                    format!("udp://127.0.0.1:{tcp_disc}").parse().unwrap(),
                ],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_tcp = tcp_listener.monitor().register::<PeerPoolEvent>();
        tcp_listener.run().await.expect("tcp_listener run");

        let tcp_client = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{tcp_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );
        tcp_client.run().await.expect("tcp_client run");

        let ev = pool_tcp.recv().await.unwrap();
        assert_eq!(ev.event, "NewPeer");
        assert_eq!(tcp_listener.peers().await, 1);

        // TLS pair
        let tls_port = free_port();
        let tls_disc = free_port();

        let tls_listener = create_node(
            Config {
                listen_endpoints: vec![format!("tls://127.0.0.1:{tls_port}").parse().unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{tls_disc}").parse().unwrap(),
                    format!("udp://127.0.0.1:{tls_disc}").parse().unwrap(),
                ],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_tls = tls_listener.monitor().register::<PeerPoolEvent>();
        tls_listener.run().await.expect("tls_listener run");

        let tls_client = create_node(
            Config {
                peer_endpoints: vec![format!("tls://127.0.0.1:{tls_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );
        tls_client.run().await.expect("tls_client run");

        let ev = pool_tls.recv().await.unwrap();
        assert_eq!(ev.event, "NewPeer");
        assert_eq!(tls_listener.peers().await, 1);

        tls_client.shutdown().await;
        tls_listener.shutdown().await;
        tcp_client.shutdown().await;
        tcp_listener.shutdown().await;
    });
}

/// Test: A single node listens on both TCP and TLS endpoints simultaneously.
/// Two clients connect, one via TCP and one via TLS.
#[test]
fn node_listens_on_multiple_transports() {
    run_test_with_executor(30, |ex| async move {
        let tcp_port = free_port();
        let tls_port = free_port();
        let disc_port = free_port();

        let server = create_node(
            Config {
                listen_endpoints: vec![
                    format!("tcp://127.0.0.1:{tcp_port}").parse().unwrap(),
                    format!("tls://127.0.0.1:{tls_port}").parse().unwrap(),
                ],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{disc_port}").parse().unwrap(),
                    format!("udp://127.0.0.1:{disc_port}").parse().unwrap(),
                ],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool = server.monitor().register::<PeerPoolEvent>();
        server.run().await.expect("server run");

        // Client connecting via TCP
        let tcp_client = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{tcp_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );
        let pool_tcp = tcp_client.monitor().register::<PeerPoolEvent>();
        tcp_client.run().await.expect("tcp_client run");

        loop {
            let ev = pool_tcp.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }

        // Client connecting via TLS
        let tls_client = create_node(
            Config {
                peer_endpoints: vec![format!("tls://127.0.0.1:{tls_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );
        let pool_tls = tls_client.monitor().register::<PeerPoolEvent>();
        tls_client.run().await.expect("tls_client run");

        loop {
            let ev = pool_tls.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }

        // Server should see 2 inbound peers
        let mut new_peers = 0;
        while new_peers < 2 {
            let ev = pool.recv().await.unwrap();
            if ev.event == "NewPeer" {
                new_peers += 1;
            }
        }

        assert_eq!(server.peers().await, 2);
        assert_eq!(server.inbound_peers().await.len(), 2);

        tls_client.shutdown().await;
        tcp_client.shutdown().await;
        server.shutdown().await;
    });
}
