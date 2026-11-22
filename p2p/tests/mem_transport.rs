//! Two nodes on a single-thread user runtime, connected through an
//! in-memory transport: no OS sockets, so it also runs on wasm32-wasip2
//! (under wasmtime in CI).

#![cfg(feature = "tokio")]

#[path = "../examples/support/mem_transport.rs"]
mod mem_transport;
#[path = "../examples/support/one_thread.rs"]
mod one_thread;

use std::sync::Arc;

use karyon_p2p::{
    ConfigBuilder, Node, PeerEvent,
    endpoint::Endpoint,
    keypair::{KeyPair, KeyPairType},
    transport::TransportRef,
};

use mem_transport::MemTransport;
use one_thread::run;

#[test]
fn nodes_connect_in_memory_on_user_runtime() {
    run(|ex| async move {
        let transport: TransportRef = Arc::new(MemTransport::new());
        let endpoint: Endpoint = "mem://node-a".parse().unwrap();

        let node_a = Node::new(
            &KeyPair::generate(&KeyPairType::Ed25519),
            ConfigBuilder::new()
                .with_listen_endpoints(vec![endpoint.clone()])
                .with_transport(transport.clone())
                .build(),
            ex.clone(),
        );
        let events_a = node_a.register_peer_events();
        node_a.run().await.unwrap();

        let node_b = Node::new(
            &KeyPair::generate(&KeyPairType::Ed25519),
            ConfigBuilder::new()
                .with_dial_peers(vec![endpoint])
                .with_transport(transport)
                .build(),
            ex,
        );
        let events_b = node_b.register_peer_events();
        node_b.run().await.unwrap();

        match events_a.recv().await.unwrap() {
            PeerEvent::Added(pid) => assert_eq!(&pid, node_b.peer_id()),
            ev => panic!("expected Added on A, got {ev:?}"),
        }
        match events_b.recv().await.unwrap() {
            PeerEvent::Added(pid) => assert_eq!(&pid, node_a.peer_id()),
            ev => panic!("expected Added on B, got {ev:?}"),
        }
        assert_eq!(node_a.peers().await, 1);
        assert_eq!(node_b.peers().await, 1);

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}
