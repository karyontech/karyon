//! Tests for direct transport + framed usage (replaces old builder tests).

use karyon_core::testing::run_test;
use karyon_net::{codec::LengthCodec, framed, tcp, Endpoint};

#[test]
fn tcp_framed_roundtrip() {
    run_test(10, async {
        let ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();
        let listener = tcp::TcpListener::bind(&ep, Default::default())
            .await
            .unwrap();
        let server_ep = listener.local_endpoint().unwrap();

        karyon_core::async_runtime::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let mut conn = framed(stream, LengthCodec::default());
            let msg: Vec<u8> = conn.recv_msg().await.unwrap();
            assert_eq!(msg, b"hello builder");
            conn.send_msg(b"hello back".to_vec()).await.unwrap();
        })
        .detach();

        let stream = tcp::connect(&server_ep, Default::default()).await.unwrap();
        let mut conn = framed(stream, LengthCodec::default());
        conn.send_msg(b"hello builder".to_vec()).await.unwrap();
        let resp: Vec<u8> = conn.recv_msg().await.unwrap();
        assert_eq!(resp, b"hello back");
    });
}

#[test]
fn tcp_framed_multiple_clients() {
    run_test(10, async {
        let ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();
        let listener = tcp::TcpListener::bind(&ep, Default::default())
            .await
            .unwrap();
        let server_ep = listener.local_endpoint().unwrap();

        karyon_core::async_runtime::spawn(async move {
            for _ in 0..5u32 {
                let stream = listener.accept().await.unwrap();
                karyon_core::async_runtime::spawn(async move {
                    let mut conn = framed(stream, LengthCodec::default());
                    let msg: Vec<u8> = conn.recv_msg().await.unwrap();
                    conn.send_msg(msg).await.unwrap();
                })
                .detach();
            }
        })
        .detach();

        for i in 0..5u32 {
            let stream = tcp::connect(&server_ep, Default::default()).await.unwrap();
            let mut conn = framed(stream, LengthCodec::default());
            let msg = format!("client-{i}").into_bytes();
            conn.send_msg(msg.clone()).await.unwrap();
            let resp: Vec<u8> = conn.recv_msg().await.unwrap();
            assert_eq!(resp, msg);
        }
    });
}

#[cfg(all(feature = "unix", target_family = "unix"))]
#[test]
fn unix_framed_roundtrip() {
    run_test(10, async {
        let path = format!("/tmp/karyon_test_builder_{}.sock", std::process::id());
        let _ = std::fs::remove_file(&path);

        let ep: Endpoint = format!("unix:{path}").parse().unwrap();
        let listener = karyon_net::unix::UnixListener::bind(&ep).unwrap();

        karyon_core::async_runtime::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let mut conn = framed(stream, LengthCodec::default());
            let msg: Vec<u8> = conn.recv_msg().await.unwrap();
            conn.send_msg(msg).await.unwrap();
        })
        .detach();

        let stream = karyon_net::unix::connect(&ep).await.unwrap();
        let mut conn = framed(stream, LengthCodec::default());
        conn.send_msg(b"unix hello".to_vec()).await.unwrap();
        let resp: Vec<u8> = conn.recv_msg().await.unwrap();
        assert_eq!(resp, b"unix hello");

        let _ = std::fs::remove_file(&path);
    });
}
