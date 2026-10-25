mod shared;

use std::{sync::Arc, time::Duration};

use async_channel::Sender;
use async_trait::async_trait;

use karyon_core::{async_util::sleep, testing::run_test_with_executor};

use karyon_p2p::{
    endpoint::Endpoint,
    monitor::PeerPoolEvent,
    protocol::{PeerConn, Protocol, ProtocolID},
    Config, Error, Version,
};

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
                dial_peers: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
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
                dial_peers: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
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
                    dial_peers: vec![connect_ep.clone()],
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

/// Config with ping too slow to detect anything during a test.
fn no_ping_config() -> Config {
    Config {
        ping_interval: 600,
        ..fast_config()
    }
}

/// Waits for messages until the peer shuts down.
async fn idle(peer: &PeerConn) -> Result<(), Error> {
    loop {
        match peer.recv().await {
            Ok(_) => {}
            Err(Error::PeerShutdown) => return Ok(()),
            Err(e) => return Err(e),
        }
    }
}

struct Alpha {
    peer: PeerConn,
}

#[async_trait]
impl Protocol for Alpha {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        idle(&self.peer).await
    }

    fn version() -> Result<Version, Error> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "Alpha".into()
    }
}

struct Beta {
    peer: PeerConn,
}

#[async_trait]
impl Protocol for Beta {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        idle(&self.peer).await
    }

    fn version() -> Result<Version, Error> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "Beta".into()
    }
}

/// Runs as "Alpha" but sends one message tagged "Beta", which this
/// node never negotiated.
struct BetaSmuggler {
    peer: PeerConn,
}

#[async_trait]
impl Protocol for BetaSmuggler {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        self.peer.inner().send("Beta".into(), b"x".to_vec()).await?;
        idle(&self.peer).await
    }

    fn version() -> Result<Version, Error> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "Alpha".into()
    }
}

#[test]
fn non_negotiated_protocol_msg_disconnects_peer() {
    run_test_with_executor(15, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();

        // A has Beta attached, but only Alpha is negotiated with B.
        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{discovery_port}").parse().unwrap(),
                    format!("udp://127.0.0.1:{discovery_port}").parse().unwrap(),
                ],
                ..no_ping_config()
            },
            ex.clone(),
        );
        node_a.attach_protocol(|peer| Alpha { peer }).await.unwrap();
        node_a.attach_protocol(|peer| Beta { peer }).await.unwrap();
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        let node_b = create_node(
            Config {
                dial_peers: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                ..no_ping_config()
            },
            ex.clone(),
        );
        node_b
            .attach_protocol(|peer| BetaSmuggler { peer })
            .await
            .unwrap();
        node_b.run().await.expect("node_b run");

        loop {
            let ev = pool_a.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }

        loop {
            let ev = pool_a.recv().await.unwrap();
            if ev.event == "RemovePeer" {
                break;
            }
        }

        assert_eq!(node_a.peers().await, 0);
        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

/// Sends one message on start, then idles. Reports on `started`.
struct Chatty {
    peer: PeerConn,
    started: Sender<()>,
}

#[async_trait]
impl Protocol for Chatty {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        let _ = self.started.send(()).await;
        self.peer.send(b"hi".to_vec()).await?;
        idle(&self.peer).await
    }

    fn version() -> Result<Version, Error> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "Chatty".into()
    }
}

#[test]
fn dialer_extra_protocol_not_run() {
    run_test_with_executor(15, |ex| async move {
        let listen_port = free_port();
        let discovery_port = free_port();
        let (started, started_rx) = async_channel::unbounded();

        let node_a = create_node(
            Config {
                listen_endpoints: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                discovery_endpoints: vec![
                    format!("tcp://127.0.0.1:{discovery_port}").parse().unwrap(),
                    format!("udp://127.0.0.1:{discovery_port}").parse().unwrap(),
                ],
                ..no_ping_config()
            },
            ex.clone(),
        );
        node_a.attach_protocol(|peer| Alpha { peer }).await.unwrap();
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        // Only B has Chatty, so B must not run it and send its message.
        let node_b = create_node(
            Config {
                dial_peers: vec![format!("tcp://127.0.0.1:{listen_port}").parse().unwrap()],
                ..no_ping_config()
            },
            ex.clone(),
        );
        node_b.attach_protocol(|peer| Alpha { peer }).await.unwrap();
        node_b
            .attach_protocol(move |peer| Chatty {
                peer,
                started: started.clone(),
            })
            .await
            .unwrap();
        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        node_b.run().await.expect("node_b run");

        for pool in [&pool_a, &pool_b] {
            loop {
                let ev = pool.recv().await.unwrap();
                if ev.event == "NewPeer" {
                    break;
                }
            }
        }

        sleep(Duration::from_secs(2)).await;
        assert!(started_rx.is_empty());
        assert_eq!(node_a.peers().await, 1);
        assert_eq!(node_b.peers().await, 1);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}
