#![cfg(feature = "quic")]

mod shared;

use std::{collections::HashSet, sync::Arc, time::Duration};

use async_channel::Sender;
use async_trait::async_trait;

use karyon_core::async_runtime::Executor;
use karyon_core::async_util::sleep;
use karyon_core::testing::{run_test, run_test_with_executor};
use karyon_net::{
    quic::{self, ClientQuicConfig, ServerQuicConfig},
    Endpoint,
};
use karyon_p2p::{
    monitor::PeerPoolEvent,
    protocol::{PeerConn, Protocol, ProtocolID},
    Config, Error, Node, Version,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use shared::{create_node, fast_config, free_port, generate_test_certs};

fn test_quic_configs() -> (ServerQuicConfig, ClientQuicConfig) {
    let (cert_chain, private_key): (Vec<CertificateDer>, std::sync::Arc<PrivateKeyDer>) =
        generate_test_certs();
    let server = ServerQuicConfig::new(cert_chain.clone(), private_key);
    let client = ClientQuicConfig::new(cert_chain, "localhost");
    (server, client)
}

#[test]
fn basic_bidirectional_stream() {
    run_test(10, async {
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let endpoint: Endpoint = "quic://127.0.0.1:0".parse().unwrap();
        let quic_endpoint = quic::QuicEndpoint::listen(&endpoint, srv_cfg)
            .await
            .unwrap();
        let local_ep = quic_endpoint.local_endpoint().unwrap();

        let server = karyon_core::async_runtime::spawn(async move {
            let conn = quic_endpoint.accept().await.unwrap();
            let (mut send, mut recv) = conn.accept_bi().await.unwrap();

            let mut buf = [0u8; 64];
            let n = recv.read(&mut buf).await.unwrap().unwrap();
            send.write_all(&buf[..n]).await.unwrap();
            send.finish().unwrap();
            sleep(Duration::from_secs(1)).await;
        });

        let conn = quic::QuicEndpoint::dial(&local_ep, cli_cfg).await.unwrap();

        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        send.write_all(b"hello quic").await.unwrap();
        send.finish().unwrap();

        let mut buf = [0u8; 64];
        let n = recv.read(&mut buf).await.unwrap().unwrap();
        assert_eq!(&buf[..n], b"hello quic");

        let _ = server.await;
    });
}

#[test]
fn multiple_streams() {
    run_test(10, async {
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let endpoint: Endpoint = "quic://127.0.0.1:0".parse().unwrap();
        let quic_endpoint = quic::QuicEndpoint::listen(&endpoint, srv_cfg)
            .await
            .unwrap();
        let local_ep = quic_endpoint.local_endpoint().unwrap();

        let num_streams = 5;

        let server = karyon_core::async_runtime::spawn(async move {
            let conn = quic_endpoint.accept().await.unwrap();
            for _ in 0..num_streams {
                let (mut send, mut recv) = conn.accept_bi().await.unwrap();
                karyon_core::async_runtime::spawn(async move {
                    let mut buf = [0u8; 256];
                    let n = recv.read(&mut buf).await.unwrap().unwrap();
                    send.write_all(&buf[..n]).await.unwrap();
                    send.finish().unwrap();
                })
                .detach();
            }
            sleep(Duration::from_secs(2)).await;
        });

        let conn = quic::QuicEndpoint::dial(&local_ep, cli_cfg).await.unwrap();

        let mut handles = Vec::new();
        for i in 0..num_streams {
            let (mut send, mut recv) = conn.open_bi().await.unwrap();
            let msg = format!("stream-{i}");
            let handle = karyon_core::async_runtime::spawn(async move {
                send.write_all(msg.as_bytes()).await.unwrap();
                send.finish().unwrap();

                let mut buf = [0u8; 256];
                let n = recv.read(&mut buf).await.unwrap().unwrap();
                assert_eq!(&buf[..n], msg.as_bytes());
            });
            handles.push(handle);
        }

        for h in handles {
            let _ = h.await;
        }

        let _ = server.await;
    });
}

#[test]
fn multiple_clients() {
    run_test(15, async {
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let endpoint: Endpoint = "quic://127.0.0.1:0".parse().unwrap();
        let quic_endpoint = quic::QuicEndpoint::listen(&endpoint, srv_cfg)
            .await
            .unwrap();
        let local_ep = quic_endpoint.local_endpoint().unwrap();

        let num_clients = 10;

        let server = karyon_core::async_runtime::spawn(async move {
            for _ in 0..num_clients {
                let conn = quic_endpoint.accept().await.unwrap();
                karyon_core::async_runtime::spawn(async move {
                    let (mut send, mut recv) = conn.accept_bi().await.unwrap();
                    let mut buf = [0u8; 256];
                    let n = recv.read(&mut buf).await.unwrap().unwrap();
                    send.write_all(&buf[..n]).await.unwrap();
                    send.finish().unwrap();
                    sleep(Duration::from_secs(2)).await;
                })
                .detach();
            }
            sleep(Duration::from_secs(5)).await;
        });

        let mut handles = Vec::new();
        for i in 0..num_clients {
            let local_ep = local_ep.clone();
            let cli_cfg = cli_cfg.clone();
            let handle = karyon_core::async_runtime::spawn(async move {
                let conn = quic::QuicEndpoint::dial(&local_ep, cli_cfg).await.unwrap();

                let (mut send, mut recv) = conn.open_bi().await.unwrap();
                let msg = format!("client-{i}");
                send.write_all(msg.as_bytes()).await.unwrap();
                send.finish().unwrap();

                let mut buf = [0u8; 256];
                let n = recv.read(&mut buf).await.unwrap().unwrap();
                assert_eq!(&buf[..n], msg.as_bytes());
            });
            handles.push(handle);
        }

        for h in handles {
            let _ = h.await;
        }

        let _ = server.await;
    });
}

#[test]
fn aggressive_load() {
    run_test(30, async {
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let endpoint: Endpoint = "quic://127.0.0.1:0".parse().unwrap();
        let quic_endpoint = quic::QuicEndpoint::listen(&endpoint, srv_cfg)
            .await
            .unwrap();
        let local_ep = quic_endpoint.local_endpoint().unwrap();

        let num_clients = 20;
        let streams_per_client = 10;

        let server = karyon_core::async_runtime::spawn(async move {
            for _ in 0..num_clients {
                let conn = quic_endpoint.accept().await.unwrap();
                karyon_core::async_runtime::spawn(async move {
                    for _ in 0..streams_per_client {
                        let (mut send, mut recv) = conn.accept_bi().await.unwrap();
                        karyon_core::async_runtime::spawn(async move {
                            let mut buf = [0u8; 1024];
                            let n = recv.read(&mut buf).await.unwrap().unwrap();
                            send.write_all(&buf[..n]).await.unwrap();
                            send.finish().unwrap();
                        })
                        .detach();
                    }
                    sleep(Duration::from_secs(5)).await;
                })
                .detach();
            }
            sleep(Duration::from_secs(10)).await;
        });

        let mut client_handles = Vec::new();
        for client_id in 0..num_clients {
            let local_ep = local_ep.clone();
            let cli_cfg = cli_cfg.clone();
            let handle = karyon_core::async_runtime::spawn(async move {
                let conn = quic::QuicEndpoint::dial(&local_ep, cli_cfg).await.unwrap();

                let mut stream_handles = Vec::new();
                for stream_id in 0..streams_per_client {
                    let (mut send, mut recv) = conn.open_bi().await.unwrap();
                    let handle = karyon_core::async_runtime::spawn(async move {
                        let msg = format!("c{client_id}-s{stream_id}");
                        send.write_all(msg.as_bytes()).await.unwrap();
                        send.finish().unwrap();

                        let mut buf = [0u8; 256];
                        let n = recv.read(&mut buf).await.unwrap().unwrap();
                        assert_eq!(&buf[..n], msg.as_bytes());
                    });
                    stream_handles.push(handle);
                }

                for h in stream_handles {
                    let _ = h.await;
                }
            });
            client_handles.push(handle);
        }

        for h in client_handles {
            let _ = h.await;
        }

        let _ = server.await;
    });
}

// -- Node over QUIC ----------------------------------------------------------

fn quic_ep(port: u16) -> Endpoint {
    format!("quic://127.0.0.1:{port}").parse().unwrap()
}

/// Node listening on QUIC at `port`, with TCP/UDP discovery on a free port.
fn quic_listener(port: u16, config: Config, ex: Executor) -> Arc<Node> {
    let discovery_port = free_port();
    create_node(
        Config {
            listen_endpoints: vec![quic_ep(port)],
            discovery_endpoints: vec![
                format!("tcp://127.0.0.1:{discovery_port}").parse().unwrap(),
                format!("udp://127.0.0.1:{discovery_port}").parse().unwrap(),
            ],
            ..config
        },
        ex,
    )
}

/// Node that only dials `port` over QUIC.
fn quic_dialer(port: u16, config: Config, ex: Executor) -> Arc<Node> {
    create_node(
        Config {
            dial_peers: vec![quic_ep(port)],
            ..config
        },
        ex,
    )
}

/// Config with ping too slow to detect anything during a test.
fn no_ping_config() -> Config {
    Config {
        ping_interval: 600,
        ..fast_config()
    }
}

/// Received messages, tagged with the protocol they arrived on.
type Inbox = Sender<(ProtocolID, Vec<u8>)>;

/// Sends the protocol id as a greeting, then reports every message it
/// receives to `inbox` until the peer shuts down.
async fn greet_and_report(peer: &PeerConn, id: ProtocolID, inbox: &Inbox) -> Result<(), Error> {
    peer.send(id.as_bytes().to_vec()).await?;
    loop {
        match peer.recv().await {
            Ok(msg) => {
                let _ = inbox.send((id.clone(), msg)).await;
            }
            Err(Error::PeerShutdown) => return Ok(()),
            Err(e) => return Err(e),
        }
    }
}

struct AlphaProtocol {
    peer: PeerConn,
    inbox: Inbox,
}

#[async_trait]
impl Protocol for AlphaProtocol {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        greet_and_report(&self.peer, Self::id(), &self.inbox).await
    }

    fn version() -> Result<Version, Error> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "Alpha".into()
    }
}

struct BetaProtocol {
    peer: PeerConn,
    inbox: Inbox,
}

#[async_trait]
impl Protocol for BetaProtocol {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        greet_and_report(&self.peer, Self::id(), &self.inbox).await
    }

    fn version() -> Result<Version, Error> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "Beta".into()
    }
}

/// Attach both test protocols to `node`, reporting into `inbox`.
async fn attach_test_protocols(node: &Node, inbox: Inbox) {
    let tx = inbox.clone();
    node.attach_protocol(move |peer| AlphaProtocol {
        peer,
        inbox: tx.clone(),
    })
    .await
    .unwrap();
    node.attach_protocol(move |peer| BetaProtocol {
        peer,
        inbox: inbox.clone(),
    })
    .await
    .unwrap();
}

#[test]
fn node_two_nodes_direct_connect() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();

        let node_a = quic_listener(listen_port, fast_config(), ex.clone());
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        let node_b = quic_dialer(listen_port, fast_config(), ex.clone());
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
fn node_protocol_messages() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();

        let node_a = quic_listener(listen_port, fast_config(), ex.clone());
        let (inbox_a, received_a) = async_channel::unbounded();
        attach_test_protocols(&node_a, inbox_a).await;
        node_a.run().await.expect("node_a run");

        let node_b = quic_dialer(listen_port, fast_config(), ex.clone());
        let (inbox_b, received_b) = async_channel::unbounded();
        attach_test_protocols(&node_b, inbox_b).await;
        node_b.run().await.expect("node_b run");

        // Each side gets the other's greeting on each protocol's own stream.
        let expected: HashSet<(ProtocolID, Vec<u8>)> = [
            ("Alpha".to_string(), b"Alpha".to_vec()),
            ("Beta".to_string(), b"Beta".to_vec()),
        ]
        .into();
        for received in [received_a, received_b] {
            let mut got = HashSet::new();
            while got.len() < expected.len() {
                got.insert(received.recv().await.unwrap());
            }
            assert_eq!(got, expected);
        }

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

#[test]
fn node_peer_disconnect_detection() {
    run_test_with_executor(30, |ex| async move {
        let listen_port = free_port();

        let node_a = quic_listener(listen_port, fast_config(), ex.clone());
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        let node_b = quic_dialer(listen_port, fast_config(), ex.clone());
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
fn node_multiple_peers_connect() {
    run_test_with_executor(60, |ex| async move {
        let listen_port = free_port();

        let server = quic_listener(listen_port, fast_config(), ex.clone());
        let pool_server = server.monitor().register::<PeerPoolEvent>();
        server.run().await.expect("server run");

        let mut clients = Vec::new();
        for i in 0..3 {
            let client = quic_dialer(listen_port, fast_config(), ex.clone());
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

/// "Stall" protocol at version 1.0.0, accepting only ^1.0.0. Reports on
/// `started` when it runs.
struct StallV1 {
    peer: PeerConn,
    started: Sender<()>,
}

#[async_trait]
impl Protocol for StallV1 {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        let _ = self.started.send(()).await;
        idle(&self.peer).await
    }

    fn version() -> Result<Version, Error> {
        "1.0.0".parse()
    }

    fn id() -> ProtocolID {
        "Stall".into()
    }
}

/// "Stall" protocol at version 2.0.0, accepting >=1.0.0. It accepts
/// StallV1 but StallV1 does not accept it, so it must not be shared.
struct StallV2 {
    peer: PeerConn,
    started: Sender<()>,
}

#[async_trait]
impl Protocol for StallV2 {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        let _ = self.started.send(()).await;
        idle(&self.peer).await
    }

    fn version() -> Result<Version, Error> {
        "2.0.0, >=1.0.0".parse()
    }

    fn id() -> ProtocolID {
        "Stall".into()
    }
}

#[test]
fn node_disconnect_detected_without_ping() {
    run_test_with_executor(15, |ex| async move {
        let listen_port = free_port();

        let node_a = quic_listener(listen_port, no_ping_config(), ex.clone());
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        let node_b = quic_dialer(listen_port, no_ping_config(), ex.clone());
        node_b.run().await.expect("node_b run");

        loop {
            let ev = pool_a.recv().await.unwrap();
            if ev.event == "NewPeer" {
                break;
            }
        }

        // Ping is off, so only the closed QUIC streams can tell A.
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
fn node_dialer_extra_protocol_not_run() {
    run_test_with_executor(15, |ex| async move {
        let listen_port = free_port();
        let (started, started_rx) = async_channel::unbounded();

        let node_a = quic_listener(listen_port, no_ping_config(), ex.clone());
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        // Only B has "Stall", so B must not open a stream for it.
        let node_b = quic_dialer(listen_port, no_ping_config(), ex.clone());
        node_b
            .attach_protocol(move |peer| StallV1 {
                peer,
                started: started.clone(),
            })
            .await
            .unwrap();
        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        node_b.run().await.expect("node_b run");

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

        sleep(Duration::from_secs(2)).await;
        assert!(started_rx.is_empty());
        assert_eq!(node_a.peers().await, 1);
        assert_eq!(node_b.peers().await, 1);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}

#[test]
fn node_one_way_version_match_not_shared() {
    run_test_with_executor(15, |ex| async move {
        let listen_port = free_port();
        let (started, started_rx) = async_channel::unbounded();

        // A accepts B's "Stall" version but B does not accept A's, so
        // neither side may run it.
        let node_a = quic_listener(listen_port, no_ping_config(), ex.clone());
        let started_a = started.clone();
        node_a
            .attach_protocol(move |peer| StallV2 {
                peer,
                started: started_a.clone(),
            })
            .await
            .unwrap();
        let pool_a = node_a.monitor().register::<PeerPoolEvent>();
        node_a.run().await.expect("node_a run");

        let node_b = quic_dialer(listen_port, no_ping_config(), ex.clone());
        node_b
            .attach_protocol(move |peer| StallV1 {
                peer,
                started: started.clone(),
            })
            .await
            .unwrap();
        let pool_b = node_b.monitor().register::<PeerPoolEvent>();
        node_b.run().await.expect("node_b run");

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

        sleep(Duration::from_secs(2)).await;
        assert!(started_rx.is_empty());
        assert_eq!(node_a.peers().await, 1);
        assert_eq!(node_b.peers().await, 1);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}
