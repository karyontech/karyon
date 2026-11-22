//! Two nodes on a runtime and a transport you supply, as on WASI: every
//! karyon task runs on the main thread, the timers are yours, and the
//! nodes connect through in-memory pipes instead of OS sockets. See
//! `support/one_thread.rs` and `support/mem_transport.rs`.
//!
//! ```text
//! cargo run -p karyon_p2p --example custom_runtime
//!
//! # on WASI
//! CARGO_TARGET_WASM32_WASIP2_RUNNER=wasmtime cargo run -p karyon_p2p \
//!     --example custom_runtime --target wasm32-wasip2 \
//!     --no-default-features --features tokio
//! ```

#[path = "support/mem_transport.rs"]
mod mem_transport;
#[path = "support/one_thread.rs"]
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

fn main() {
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
        let events = node_a.register_peer_events();
        node_a.run().await.expect("run node A");

        let node_b = Node::new(
            &KeyPair::generate(&KeyPairType::Ed25519),
            ConfigBuilder::new()
                .with_dial_peers(vec![endpoint])
                .with_transport(transport)
                .build(),
            ex,
        );
        node_b.run().await.expect("run node B");

        if let Ok(PeerEvent::Added(pid)) = events.recv().await {
            println!("node A connected to {pid}");
        }

        node_b.shutdown().await;
        node_a.shutdown().await;
    });
}
