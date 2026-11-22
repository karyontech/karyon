use std::{net::TcpListener, sync::Arc, time::Duration};

use karyon_core::{
    async_runtime::Executor,
    async_util::{sleep, timeout},
    testing::run_test_with_executor,
};

use karyon_kademlia::{KademliaConfig, KademliaDiscovery};
use karyon_p2p::{
    Config, Node,
    endpoint::Endpoint,
    keypair::{KeyPair, KeyPairType},
};

const NODES: usize = 30;

/// `n` distinct free ports: all are held until the last one is taken.
fn free_ports(n: usize) -> Vec<u16> {
    let held: Vec<TcpListener> = (0..n)
        .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    held.iter()
        .map(|l| l.local_addr().unwrap().port())
        .collect()
}

fn ep(scheme: &str, port: u16) -> Endpoint {
    format!("{scheme}://127.0.0.1:{port}").parse().unwrap()
}

/// A running node listening on `port`, with a lookup (tcp) and a refresh
/// (udp) endpoint on `kad_port`, seeded from `bootstrap`.
async fn kad_node(port: u16, kad_port: u16, bootstrap: Vec<Endpoint>, ex: Executor) -> Arc<Node> {
    let config = Config {
        listen_endpoints: vec![ep("tcp", port)],
        ping_interval: 3,
        ping_timeout: 2,
        handshake_timeout: 5,
        ..Default::default()
    };
    let kconfig = KademliaConfig {
        discovery_endpoints: vec![ep("tcp", kad_port), ep("udp", kad_port)],
        bootstrap_peers: bootstrap,
        // A node whose first seeding fails (busy bootstrap) retries soon.
        seeding_interval: 5,
        // Peers that failed (e.g. full) are pinged and retried soon.
        refresh_interval: 10,
        ..Default::default()
    };
    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
    let node = Node::with_discovery(
        &key_pair,
        config,
        |ctx| KademliaDiscovery::new(ctx, kconfig),
        ex,
    );
    node.run().await.unwrap();
    node
}

/// Polls `node` until it has at least one peer, for at most `secs` seconds.
async fn wait_connected(node: &Node, secs: u64) {
    let poll = async {
        while node.peers().await == 0 {
            sleep(Duration::from_millis(100)).await;
        }
    };
    timeout(Duration::from_secs(secs), poll)
        .await
        .expect("node found no peer");
}

/// 29 nodes seed from one bootstrap node and all find peers. The bootstrap
/// has only `inbound_slots` (12) for them, so most must connect to peers
/// they discovered.
#[test]
fn discovery_network_of_30() {
    run_test_with_executor(180, |ex| async move {
        let ports = free_ports(2 * NODES);
        let bootstrap = kad_node(ports[0], ports[1], vec![], ex.clone()).await;

        let seed = vec![ep("tcp", ports[1])];
        let mut nodes = Vec::new();
        for i in 1..NODES {
            let node = kad_node(ports[2 * i], ports[2 * i + 1], seed.clone(), ex.clone());
            nodes.push(node.await);
        }

        for node in &nodes {
            wait_connected(node, 90).await;
        }
        let inbound = Config::default().inbound_slots;
        assert!(bootstrap.inbound_peers().await.len() <= inbound);

        for node in &nodes {
            node.shutdown().await;
        }
        bootstrap.shutdown().await;
    });
}
