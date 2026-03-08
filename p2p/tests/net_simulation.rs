use std::{sync::Arc, time::Duration};

use karyon_core::{
    async_runtime::Executor,
    async_util::{sleep, timeout},
};

use karyon_p2p::{
    endpoint::Endpoint,
    keypair::{KeyPair, KeyPairType},
    monitor::PeerPoolEvent,
    Backend, Config,
};

/// Find a free TCP port by binding to port 0 and returning the assigned port.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Config with short ping for quick disconnect detection.
fn fast_config() -> Config {
    Config {
        enable_monitor: true,
        ping_interval: 3,
        ping_timeout: 2,
        handshake_timeout: 5,
        ..Default::default()
    }
}

/// Helper to create a Backend with the given config and shared executor.
fn create_node(config: Config, ex: Executor) -> Arc<Backend> {
    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
    Backend::new(&key_pair, config, ex)
}

/// Run an async test with a multi-threaded executor and a timeout.
fn run_test<F: FnOnce(Executor) -> Fut, Fut: std::future::Future<Output = ()>>(
    timeout_secs: u64,
    f: F,
) {
    #[cfg(feature = "smol")]
    {
        let ex = Arc::new(smol::Executor::new());
        let executor: Executor = ex.clone().into();
        let (signal, shutdown) = async_channel::unbounded::<()>();

        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        let test_future = f(executor);

        easy_parallel::Parallel::new()
            .each(0..num_threads, |_| {
                smol::future::block_on(ex.run(shutdown.recv()))
            })
            .finish(|| {
                smol::future::block_on(async {
                    timeout(Duration::from_secs(timeout_secs), test_future)
                        .await
                        .expect(&format!("Test timed out after {timeout_secs}s"));
                    drop(signal);
                })
            });
    }

    #[cfg(feature = "tokio")]
    {
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime"),
        );
        let executor: Executor = rt.clone().into();
        let test_future = f(executor);

        rt.block_on(async {
            timeout(Duration::from_secs(timeout_secs), test_future)
                .await
                .expect(&format!("Test timed out after {timeout_secs}s"));
        });
    }
}

/// Test: Two nodes connect to each other via peer_endpoints (direct connection).
#[test]
fn two_nodes_direct_connect() {
    run_test(30, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();

        let node_a = create_node(
            Config {
                listen_endpoint: Some(
                    format!("tcp://127.0.0.1:{listen_port}")
                        .parse()
                        .unwrap(),
                ),
                discovery_port,
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}")
                    .parse()
                    .unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        node_b.run().await.expect("node_b run");

        // Wait for NewPeer on both sides
        let ev_a = pool_a.recv().await.unwrap();
        assert_eq!(ev_a.event, "NewPeer", "node_a should see NewPeer");

        let ev_b = pool_b.recv().await.unwrap();
        assert_eq!(ev_b.event, "NewPeer", "node_b should see NewPeer");

        assert_eq!(node_a.peers().await, 1);
        assert_eq!(node_b.peers().await, 1);

        assert_eq!(node_a.inbound_peers().await.len(), 1);
        assert_eq!(node_b.outbound_peers().await.len(), 1);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

/// Test: A small network of 4 nodes where nodes discover each other through
/// bootstrap peers and the Kademlia-based discovery protocol.
#[test]
fn four_node_discovery_network() {
    run_test(60, |ex| async move {
        let n1_listen_port = free_port();
        let n1_discovery_port = free_port();

        let node1 = create_node(
            Config {
                listen_endpoint: Some(
                    format!("tcp://127.0.0.1:{n1_listen_port}")
                        .parse()
                        .unwrap(),
                ),
                discovery_port: n1_discovery_port,
                ..fast_config()
            },
            ex.clone(),
        );

        node1.run().await.expect("node1 run");

        let bootstrap_ep: Endpoint = format!("tcp://127.0.0.1:{n1_discovery_port}")
            .parse()
            .unwrap();

        let n2_listen_port = free_port();
        let n2_discovery_port = free_port();
        let node2 = create_node(
            Config {
                listen_endpoint: Some(
                    format!("tcp://127.0.0.1:{n2_listen_port}")
                        .parse()
                        .unwrap(),
                ),
                discovery_port: n2_discovery_port,
                bootstrap_peers: vec![bootstrap_ep.clone()],
                ..fast_config()
            },
            ex.clone(),
        );

        let n3_listen_port = free_port();
        let n3_discovery_port = free_port();
        let node3 = create_node(
            Config {
                listen_endpoint: Some(
                    format!("tcp://127.0.0.1:{n3_listen_port}")
                        .parse()
                        .unwrap(),
                ),
                discovery_port: n3_discovery_port,
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

        // Wait until node4 has at least 1 peer
        loop {
            let ev = pool4.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }

        assert!(node4.peers().await >= 1, "node4 should have at least 1 peer");

        sleep(Duration::from_secs(3)).await;

        assert!(
            node1.peers().await >= 1,
            "node1 should have at least 1 peer, got {}",
            node1.peers().await
        );

        node4.shutdown().await;
        node3.shutdown().await;
        node2.shutdown().await;
        node1.shutdown().await;
    });
}

/// Test: Nodes connect and then one shuts down. The remaining node detects
/// the disconnection via ping failure.
#[test]
fn peer_disconnect_detection() {
    run_test(30, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();

        let node_a = create_node(
            Config {
                listen_endpoint: Some(
                    format!("tcp://127.0.0.1:{listen_port}")
                        .parse()
                        .unwrap(),
                ),
                discovery_port,
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}")
                    .parse()
                    .unwrap()],
                ..fast_config()
            },
            ex.clone(),
        );

        node_b.run().await.expect("node_b run");

        // Wait for NewPeer
        loop {
            let ev = pool_a.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }
        assert_eq!(node_a.peers().await, 1);

        // Shut down node_b
        node_b.shutdown().await;

        // node_a should detect the disconnection (RemovePeer event)
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

/// Test: Multiple nodes connect to the same listener.
#[test]
fn multiple_peers_connect() {
    run_test(30, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();

        let server = create_node(
            Config {
                listen_endpoint: Some(
                    format!("tcp://127.0.0.1:{listen_port}")
                        .parse()
                        .unwrap(),
                ),
                discovery_port,
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_server = server.monitor().register::<PeerPoolEvent>();
        server.run().await.expect("server run");

        let connect_ep: Endpoint = format!("tcp://127.0.0.1:{listen_port}")
            .parse()
            .unwrap();

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

            // Wait for this client to fully connect before starting the next
            loop {
                let ev = pool_client.recv().await.unwrap();
                if ev.event == "NewPeer" {
                    break;
                }
            }
            assert_eq!(client.peers().await, 1, "client {i} should have 1 peer");
            clients.push(client);
        }

        // Wait for server to see 3 NewPeer events
        let mut new_peers = 0;
        while new_peers < 3 {
            let ev = pool_server.recv().await.unwrap();
            if ev.event == "NewPeer" {
                new_peers += 1;
            }
        }

        assert_eq!(server.peers().await, 3, "server should have 3 peers");
        assert_eq!(server.inbound_peers().await.len(), 3);

        // Shutdown all clients and verify server detects disconnections
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

/// Test: Two nodes connect over TLS and successfully handshake.
#[test]
fn two_nodes_tls_connect() {
    run_test(30, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();

        let node_a = create_node(
            Config {
                listen_endpoint: Some(
                    format!("tcp://127.0.0.1:{listen_port}")
                        .parse()
                        .unwrap(),
                ),
                discovery_port,
                enable_tls: true,
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        let node_b = create_node(
            Config {
                peer_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}")
                    .parse()
                    .unwrap()],
                enable_tls: true,
                ..fast_config()
            },
            ex.clone(),
        );

        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        node_b.run().await.expect("node_b run");

        let ev_a = pool_a.recv().await.unwrap();
        assert_eq!(ev_a.event, "NewPeer", "node_a should see NewPeer over TLS");

        let ev_b = pool_b.recv().await.unwrap();
        assert_eq!(ev_b.event, "NewPeer", "node_b should see NewPeer over TLS");

        assert_eq!(node_a.peers().await, 1);
        assert_eq!(node_b.peers().await, 1);

        assert_eq!(node_a.inbound_peers().await.len(), 1);
        assert_eq!(node_b.outbound_peers().await.len(), 1);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

/// Test: Multiple nodes form a TLS-enabled network via bootstrap discovery.
#[test]
fn tls_discovery_network() {
    run_test(60, |ex| async move {
        let n1_listen_port = free_port();
        let n1_discovery_port = free_port();

        let node1 = create_node(
            Config {
                listen_endpoint: Some(
                    format!("tcp://127.0.0.1:{n1_listen_port}")
                        .parse()
                        .unwrap(),
                ),
                discovery_port: n1_discovery_port,
                enable_tls: true,
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
                listen_endpoint: Some(
                    format!("tcp://127.0.0.1:{n2_listen_port}")
                        .parse()
                        .unwrap(),
                ),
                discovery_port: n2_discovery_port,
                bootstrap_peers: vec![bootstrap_ep.clone()],
                enable_tls: true,
                ..fast_config()
            },
            ex.clone(),
        );

        let node3 = create_node(
            Config {
                bootstrap_peers: vec![bootstrap_ep],
                enable_tls: true,
                ..fast_config()
            },
            ex.clone(),
        );

        let pool3 = node3.monitor().register::<PeerPoolEvent>();

        node2.run().await.expect("node2 run");
        node3.run().await.expect("node3 run");

        // Wait for node3 to connect to at least 1 peer via TLS
        loop {
            let ev = pool3.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }
        assert!(node3.peers().await >= 1, "node3 should have at least 1 peer");

        // Wait for node1 to have at least 1 peer
        loop {
            let ev = pool1.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }
        assert!(node1.peers().await >= 1, "node1 should have at least 1 peer");

        node3.shutdown().await;
        node2.shutdown().await;
        node1.shutdown().await;
    });
}
