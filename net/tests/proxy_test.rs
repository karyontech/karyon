#![cfg(all(feature = "proxy", feature = "tcp"))]

use karyon_core::async_runtime::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use karyon_core::testing::run_test;
use karyon_net::{layers::proxy::Socks5Layer, tcp, ClientLayer, Endpoint};

/// Copy bytes from reader to writer until EOF or error.
async fn relay<R, W>(mut r: R, mut w: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = [0u8; 4096];
    loop {
        match AsyncReadExt::read(&mut r, &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if AsyncWriteExt::write_all(&mut w, &buf[..n]).await.is_err() {
                    break;
                }
                let _ = AsyncWriteExt::flush(&mut w).await;
            }
        }
    }
}

/// Minimal SOCKS5 proxy that tunnels to a local TCP echo server.
async fn run_socks5_proxy(proxy_ep: &Endpoint, target_addr: std::net::SocketAddr) -> Endpoint {
    let listener = tcp::TcpListener::bind(proxy_ep, Default::default())
        .await
        .unwrap();
    let resolved = listener.local_endpoint().unwrap();

    karyon_core::async_runtime::spawn(async move {
        loop {
            let mut client = match listener.accept().await {
                Ok(c) => c,
                Err(_) => break,
            };

            karyon_core::async_runtime::spawn(async move {
                // Read greeting
                let mut greeting = [0u8; 3];
                client.read_exact(&mut greeting).await.unwrap();
                // Reply: version 5, no auth
                client.write_all(&[0x05, 0x00]).await.unwrap();
                client.flush().await.unwrap();

                // Read connect request header
                let mut hdr = [0u8; 4];
                client.read_exact(&mut hdr).await.unwrap();

                // Skip address based on type
                match hdr[3] {
                    0x01 => {
                        // IPv4 + port
                        let mut skip = [0u8; 6];
                        client.read_exact(&mut skip).await.unwrap();
                    }
                    0x03 => {
                        // Domain
                        let mut len = [0u8; 1];
                        client.read_exact(&mut len).await.unwrap();
                        let mut skip = vec![0u8; len[0] as usize + 2];
                        client.read_exact(&mut skip).await.unwrap();
                    }
                    _ => return,
                }

                // Connect to target
                let target = karyon_core::async_runtime::net::TcpStream::connect(target_addr)
                    .await
                    .unwrap();

                // Reply success with dummy bind addr
                client
                    .write_all(&[
                        0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, // bind addr
                        0, 0, // bind port
                    ])
                    .await
                    .unwrap();
                client.flush().await.unwrap();

                // Bidirectional relay using two tasks
                let (cr, cw) = karyon_core::async_runtime::io::split(client);
                let (tr, tw) = karyon_core::async_runtime::io::split(target);

                let t1 = karyon_core::async_runtime::spawn(relay(cr, tw));
                let t2 = karyon_core::async_runtime::spawn(relay(tr, cw));

                let _ = t1.await;
                let _ = t2.await;
            })
            .detach();
        }
    })
    .detach();

    resolved
}

/// Simple TCP echo server.
async fn run_echo_server(ep: &Endpoint) -> (Endpoint, std::net::SocketAddr) {
    let listener = tcp::TcpListener::bind(ep, Default::default())
        .await
        .unwrap();
    let resolved = listener.local_endpoint().unwrap();
    let addr: std::net::SocketAddr = resolved.clone().try_into().unwrap();

    karyon_core::async_runtime::spawn(async move {
        loop {
            let mut conn = match listener.accept().await {
                Ok(c) => c,
                Err(_) => break,
            };
            karyon_core::async_runtime::spawn(async move {
                let mut buf = [0u8; 256];
                loop {
                    let n = match conn.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    if conn.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                    let _ = conn.flush().await;
                }
            })
            .detach();
        }
    })
    .detach();

    (resolved, addr)
}

#[test]
fn socks5_tunnel_echo() {
    run_test(10, async {
        let echo_ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();
        let (_, echo_addr) = run_echo_server(&echo_ep).await;

        let proxy_ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();
        let proxy_resolved = run_socks5_proxy(&proxy_ep, echo_addr).await;

        // Connect to proxy, then tunnel to echo server via SOCKS5
        let stream = tcp::connect(&proxy_resolved, Default::default())
            .await
            .unwrap();

        let addr_str = echo_addr.ip().to_string();
        let layer = Socks5Layer::new(&addr_str, echo_addr.port());
        let mut tunneled = ClientLayer::handshake(&layer, stream).await.unwrap();

        // Send and receive through the tunnel
        tunneled.write_all(b"hello proxy").await.unwrap();
        tunneled.flush().await.unwrap();

        let mut buf = [0u8; 64];
        let n = tunneled.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello proxy");
    });
}

#[test]
fn socks5_tunnel_domain() {
    run_test(10, async {
        let echo_ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();
        let (_, echo_addr) = run_echo_server(&echo_ep).await;

        let proxy_ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();
        let proxy_resolved = run_socks5_proxy(&proxy_ep, echo_addr).await;

        // Use domain name "localhost" which the mock proxy
        // ignores (connects to target_addr directly)
        let stream = tcp::connect(&proxy_resolved, Default::default())
            .await
            .unwrap();

        let layer = Socks5Layer::new("localhost", echo_addr.port());
        let mut tunneled = ClientLayer::handshake(&layer, stream).await.unwrap();

        tunneled.write_all(b"domain test").await.unwrap();
        tunneled.flush().await.unwrap();

        let mut buf = [0u8; 64];
        let n = tunneled.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"domain test");
    });
}

#[test]
fn socks5_multiple_tunnels() {
    run_test(10, async {
        let echo_ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();
        let (_, echo_addr) = run_echo_server(&echo_ep).await;

        let proxy_ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();
        let proxy_resolved = run_socks5_proxy(&proxy_ep, echo_addr).await;

        let addr_str = echo_addr.ip().to_string();

        for i in 0..5u32 {
            let stream = tcp::connect(&proxy_resolved, Default::default())
                .await
                .unwrap();

            let layer = Socks5Layer::new(&addr_str, echo_addr.port());
            let mut tunneled = ClientLayer::handshake(&layer, stream).await.unwrap();

            let msg = format!("msg-{i}");
            tunneled.write_all(msg.as_bytes()).await.unwrap();
            tunneled.flush().await.unwrap();

            let mut buf = [0u8; 64];
            let n = tunneled.read(&mut buf).await.unwrap();
            assert_eq!(&buf[..n], msg.as_bytes());
        }
    });
}
