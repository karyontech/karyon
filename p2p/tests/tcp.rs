mod shared;

use karyon_core::testing::run_test_with_executor;

use karyon_p2p::{endpoint::Endpoint, monitor::PeerPoolEvent, Config};

use shared::{create_node, fast_config, free_port};

#[test]
fn two_nodes_direct_connect() {
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
        node_a.run().await.expect("node_a run");

        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
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
fn four_node_discovery_network() {
    run_test_with_executor(60, |ex| async move {
        let n1_listen_port = free_port();
        let n1_discovery_port = free_port();

        let node1 = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{n1_listen_port}")
                    .parse()
                    .unwrap()],
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
                listen_endpoints: vec![format!("tcp://127.0.0.1:{n2_listen_port}")
                    .parse()
                    .unwrap()],
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

        let n3_listen_port = free_port();
        let n3_discovery_port = free_port();
        let node3 = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{n3_listen_port}")
                    .parse()
                    .unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{n3_discovery_port}")
                        .parse()
                        .unwrap(),
                    format!("udp://127.0.0.1:{n3_discovery_port}")
                        .parse()
                        .unwrap(),
                ],
                bootstrap_peers: vec![bootstrap_ep.clone()],
                ..fast_config()
            },
            ex.clone(),
        );

        let node4 = create_node(
            Config {
                bootstrap_peers: vec![bootstrap_ep],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool4 = node4.monitor().register::<PeerPoolEvent>();

        node2.run().await.expect("node2 run");
        node3.run().await.expect("node3 run");
        node4.run().await.expect("node4 run");

        loop {
            let ev = pool4.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }

        assert!(node4.peers().await >= 1);

        loop {
            let ev = pool1.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }
        assert!(node1.peers().await >= 1);

        node4.shutdown().await;
        node3.shutdown().await;
        node2.shutdown().await;
        node1.shutdown().await;
    });
}

#[test]
fn peer_disconnect_detection() {
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
        node_a.run().await.expect("node_a run");

        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );

        node_b.run().await.expect("node_b run");

        loop {
            let ev = pool_a.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }
        assert_eq!(node_a.peers().await, 1);

        node_b.shutdown().await;

        loop {
            let ev = pool_a.recv().await.unwrap();
            if ev.event == "RemovePeer" {
                break;
            }
        }

        assert_eq!(node_a.peers().await, 0);
        node_a.shutdown().await;
    });
}

#[test]
fn multiple_peers_connect() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();

        let server = create_node(
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

        let pool_server = server.monitor().register::<PeerPoolEvent>();
        server.run().await.expect("server run");

        let connect_ep: Endpoint = format!("tcp://127.0.0.1:{listen_port}").parse().unwrap();

        let mut clients = Vec::new();
        for i in 0..3 {
            let client = create_node(
                Config {
                    peer_endpoints: vec![connect_ep.clone()],
                    ..fast_config()
                },
                ex.clone(),
            );
            let pool_client = client.monitor().register::<PeerPoolEvent>();
            client.run().await.expect("client run");

            loop {
                let ev = pool_client.recv().await.unwrap();
                if ev.event == "NewPeer" {
                    break;
                }
            }
            assert_eq!(client.peers().await, 1, "client {i} should have 1 peer");
            clients.push(client);
        }

        let mut new_peers = 0;
        while new_peers < 3 {
            let ev = pool_server.recv().await.unwrap();
            if ev.event == "NewPeer" {
                new_peers += 1;
            }
        }

        assert_eq!(server.peers().await, 3);
        assert_eq!(server.inbound_peers().await.len(), 3);

        for client in &clients {
            client.shutdown().await;
        }

        let mut removed = 0;
        while removed < 3 {
            let ev = pool_server.recv().await.unwrap();
            if ev.event == "RemovePeer" {
                removed += 1;
            }
        }

        assert_eq!(server.peers().await, 0);
        server.shutdown().await;
    });
}
