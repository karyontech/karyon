use std::sync::Arc;

use karyon_core::async_runtime::Executor;

use karyon_p2p::{
    keypair::{KeyPair, KeyPairType},
    Config, Node,
};

/// Find a free TCP port by binding to port 0 and returning the assigned port.
#[allow(dead_code)]
pub fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Config with short ping for quick disconnect detection.
#[allow(dead_code)]
pub fn fast_config() -> Config {
    Config {
        enable_monitor: true,
        ping_interval: 3,
        ping_timeout: 2,
        handshake_timeout: 5,
        ..Default::default()
    }
}

/// Helper to create a Node with the given config and shared executor.
#[allow(dead_code)]
pub fn create_node(config: Config, ex: Executor) -> Arc<Node> {
    let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
    Node::new(&key_pair, config, ex)
}

/// Generate a self-signed certificate and key for testing.
#[allow(dead_code)]
pub fn generate_test_certs() -> (
    Vec<rustls_pki_types::CertificateDer<'static>>,
    Arc<rustls_pki_types::PrivateKeyDer<'static>>,
) {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_der = rustls_pki_types::CertificateDer::from(certified.cert);
    let key_der: rustls_pki_types::PrivateKeyDer<'static> = certified.signing_key.into();
    (vec![cert_der], Arc::new(key_der))
}
