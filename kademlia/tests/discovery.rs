use std::{net::TcpListener, sync::Arc};

use karyon_core::{async_runtime::Executor, testing::run_test_with_executor};

use karyon_kademlia::{KademliaConfig, KademliaDiscovery};
use karyon_p2p::{
    Config, Node,
    endpoint::Endpoint,
    keypair::{KeyPair, KeyPairType},
    monitor::PeerPoolEvent,
};

/// Find a free TCP port by binding to port 0 and returning the assigned port.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// A `scheme://127.0.0.1:<free port>` endpoint.
fn free_endpoint(scheme: &str) -> Endpoint {
    format!("{scheme}://127.0.0.1:{}", free_port())
        .parse()
        .unwrap()
}

/// Node config with short pings, listening on `listen` if given.
fn node_config(listen: Option<Endpoint>) -> Config {
    Config {
        listen_endpoints: listen.into_iter().collect(),
        enable_monitor: true,
        ping_interval: 3,
        ping_timeout: 2,
        handshake_timeout: 5,
        ..Default::default()
    }
}

/// Kademlia config with a lookup (tcp) and a refresh (udp) endpoint on
/// `port`, seeded from `bootstrap`.
fn kad_config(port: Option<u16>, bootstrap: Vec<Endpoint>) -> KademliaConfig {
    let discovery_endpoints = match port {
        Some(p) => vec![
            format!("tcp://127.0.0.1:{p}").parse().unwrap(),
            format!("udp://127.0.0.1:{p}").parse().unwrap(),
        ],
        None => vec![],
    };
    KademliaConfig {
        discovery_endpoints,
        bootstrap_peers: bootstrap,
        ..Default::default()
    }
}

fn kad_node(config: Config, kconfig: KademliaConfig, ex: Executor) -> Arc<Node> {
    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
    Node::with_discovery(
        &key_pair,
        config,
        |ctx| KademliaDiscovery::new(ctx, kconfig),
        ex,
    )
}

#[test]
fn four_node_discovery_network() {
    run_test_with_executor(60, |ex| async move {
        let n1_discovery_port = free_port();
        let node1 = kad_node(
            node_config(Some(free_endpoint("tcp"))),
            kad_config(Some(n1_discovery_port), vec![]),
            ex.clone(),
        );
        let pool1 = node1.monitor().register::<PeerPoolEvent>();
        node1.run().await.expect("node1 run");

        let bootstrap_ep: Endpoint = format!("tcp://127.0.0.1:{n1_discovery_port}")
            .parse()
            .unwrap();

        let node2 = kad_node(
            node_config(Some(free_endpoint("tcp"))),
            kad_config(Some(free_port()), vec![bootstrap_ep.clone()]),
            ex.clone(),
        );
        let node3 = kad_node(
            node_config(Some(free_endpoint("tcp"))),
            kad_config(Some(free_port()), vec![bootstrap_ep.clone()]),
            ex.clone(),
        );
        let node4 = kad_node(
            node_config(None),
            kad_config(None, vec![bootstrap_ep]),
            ex.clone(),
        );
        let pool4 = node4.monitor().register::<PeerPoolEvent>();

        node2.run().await.expect("node2 run");
        node3.run().await.expect("node3 run");
        node4.run().await.expect("node4 run");

        while pool4.recv().await.unwrap().event != "NewPeer" {}
        assert!(node4.peers().await >= 1);

        while pool1.recv().await.unwrap().event != "NewPeer" {}
        assert!(node1.peers().await >= 1);

        node4.shutdown().await;
        node3.shutdown().await;
        node2.shutdown().await;
        node1.shutdown().await;
    });
}

#[test]
fn tls_discovery_network() {
    run_test_with_executor(60, |ex| async move {
        let n1_discovery_port = free_port();
        let node1 = kad_node(
            node_config(Some(free_endpoint("tls"))),
            kad_config(Some(n1_discovery_port), vec![]),
            ex.clone(),
        );
        let pool1 = node1.monitor().register::<PeerPoolEvent>();
        node1.run().await.expect("node1 run");

        let bootstrap_ep: Endpoint = format!("tcp://127.0.0.1:{n1_discovery_port}")
            .parse()
            .unwrap();

        let node2 = kad_node(
            node_config(Some(free_endpoint("tls"))),
            kad_config(Some(free_port()), vec![bootstrap_ep.clone()]),
            ex.clone(),
        );
        let node3 = kad_node(
            node_config(None),
            kad_config(None, vec![bootstrap_ep]),
            ex.clone(),
        );
        let pool3 = node3.monitor().register::<PeerPoolEvent>();

        node2.run().await.expect("node2 run");
        node3.run().await.expect("node3 run");

        while pool3.recv().await.unwrap().event != "NewPeer" {}
        assert!(node3.peers().await >= 1);

        while pool1.recv().await.unwrap().event != "NewPeer" {}
        assert!(node1.peers().await >= 1);

        node3.shutdown().await;
        node2.shutdown().await;
        node1.shutdown().await;
    });
}
