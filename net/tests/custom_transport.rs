//! A user `Transport` (in-memory `mem://` pipes): no OS sockets, so it also
//! runs on wasm32-wasip2 (under wasmtime in CI).

#![cfg(all(feature = "tokio", feature = "framing"))]

#[path = "../examples/support/mem_transport.rs"]
mod mem_transport;

use std::sync::Arc;

use karyon_net::{
    Endpoint,
    codec::LengthCodec,
    framed,
    transport::{Transport, TransportRef},
};

use mem_transport::MemTransport;

fn endpoint(s: &str) -> Endpoint {
    s.parse().unwrap()
}

#[test]
fn supports_only_its_scheme() {
    let transport = MemTransport::new();
    assert!(transport.supports(&endpoint("mem://server")));
    assert!(!transport.supports(&endpoint("tcp://127.0.0.1:3000")));
}

#[tokio::test]
async fn framed_messages_both_ways() {
    // Used through `dyn Transport`, as crates on karyon_net hold it.
    let transport: TransportRef = Arc::new(MemTransport::new());
    let server_ep = endpoint("mem://server");

    let listener = transport.listen(&server_ep).await.unwrap();
    assert_eq!(listener.local_endpoint().unwrap(), server_ep);

    let client = transport.dial(&server_ep).await.unwrap();
    let server = listener.accept().await.unwrap();

    let mut client = framed(client, LengthCodec::default());
    let mut server = framed(server, LengthCodec::default());

    // Each side sees the other's endpoint.
    assert_eq!(client.peer_endpoint(), Some(server_ep.clone()));
    assert_eq!(client.local_endpoint(), Some(endpoint("mem://dialer")));
    assert_eq!(server.peer_endpoint(), Some(endpoint("mem://dialer")));
    assert_eq!(server.local_endpoint(), Some(server_ep));

    client.send_msg(b"ping".to_vec()).await.unwrap();
    assert_eq!(server.recv_msg().await.unwrap(), b"ping");

    server.send_msg(b"pong".to_vec()).await.unwrap();
    assert_eq!(client.recv_msg().await.unwrap(), b"pong");
}
