#![cfg(feature = "quic")]

mod shared;

use std::time::Duration;

use karyon_core::async_util::sleep;
use karyon_core::testing::run_test;
use karyon_net::{
    quic::{self, ClientQuicConfig, ServerQuicConfig},
    Endpoint,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use shared::generate_test_certs;

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
