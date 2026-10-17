use std::net::{
    TcpListener as StdTcpListener, TcpStream as StdTcpStream, UdpSocket as StdUdpSocket,
};
#[cfg(any(feature = "tls", feature = "quic"))]
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "tls")]
use karyon_net::async_rustls::rustls::crypto::aws_lc_rs::default_provider;
#[cfg(feature = "tls")]
use karyon_net::async_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};
#[cfg(any(feature = "tls", feature = "quic"))]
use rcgen::generate_simple_self_signed;
#[cfg(any(feature = "tls", feature = "quic"))]
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use karyon_core::async_util::sleep;
use karyon_core::testing::run_test_with_executor;
#[cfg(feature = "quic")]
use karyon_net::quic::{ClientQuicConfig, ServerQuicConfig};
#[cfg(feature = "tls")]
use karyon_net::tls::{ClientTlsConfig, ServerTlsConfig};
use karyon_net::Endpoint;
use karyon_stun::{StunClient, StunConfig, StunServer};

/// Find a free UDP port by binding to port 0.
fn free_udp_port() -> u16 {
    let socket = StdUdpSocket::bind("127.0.0.1:0").unwrap();
    socket.local_addr().unwrap().port()
}

/// Find a free TCP port by binding to port 0.
fn free_tcp_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Self-signed cert and key for "localhost".
#[cfg(any(feature = "tls", feature = "quic"))]
fn test_certs() -> (Vec<CertificateDer<'static>>, Arc<PrivateKeyDer<'static>>) {
    let certified = generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let key: PrivateKeyDer<'static> = certified.signing_key.into();
    (vec![CertificateDer::from(certified.cert)], Arc::new(key))
}

#[test]
fn udp_binding() {
    run_test_with_executor(15, |ex| async move {
        let port = free_udp_port();
        let endpoint: Endpoint = format!("udp://127.0.0.1:{port}").parse().unwrap();

        let config = StunConfig {
            udp_endpoint: Some(endpoint.clone()),
            software: Some("karyon-stun-test".into()),
            ..Default::default()
        };
        let server = StunServer::new(config, ex);
        server.run().await.unwrap();

        let client = StunClient::new(endpoint);
        let addr = client.discover().await.unwrap();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);

        server.shutdown().await;
    });
}

#[test]
fn udp_binding_from_fixed_local_port() {
    run_test_with_executor(15, |ex| async move {
        let port = free_udp_port();
        let endpoint: Endpoint = format!("udp://127.0.0.1:{port}").parse().unwrap();

        let config = StunConfig {
            udp_endpoint: Some(endpoint.clone()),
            ..Default::default()
        };
        let server = StunServer::new(config, ex);
        server.run().await.unwrap();

        // The mapped address must be exactly the local socket we bound.
        let local_port = free_udp_port();
        let local = format!("127.0.0.1:{local_port}").parse().unwrap();
        let client = StunClient::new(endpoint);
        let addr = client.discover_from(local).await.unwrap();
        assert_eq!(addr, local);

        server.shutdown().await;
    });
}

#[test]
fn tcp_binding() {
    run_test_with_executor(15, |ex| async move {
        let port = free_tcp_port();
        let endpoint: Endpoint = format!("tcp://127.0.0.1:{port}").parse().unwrap();

        let config = StunConfig {
            tcp_endpoint: Some(endpoint.clone()),
            software: Some("karyon-stun-test".into()),
            ..Default::default()
        };
        let server = StunServer::new(config, ex);
        server.run().await.unwrap();

        let client = StunClient::new(endpoint);
        let addr = client.discover().await.unwrap();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);

        server.shutdown().await;
    });
}

#[test]
fn tcp_connection_limit() {
    run_test_with_executor(15, |ex| async move {
        let port = free_tcp_port();
        let endpoint: Endpoint = format!("tcp://127.0.0.1:{port}").parse().unwrap();

        let config = StunConfig {
            tcp_endpoint: Some(endpoint.clone()),
            max_connections: 1,
            ..Default::default()
        };
        let server = StunServer::new(config, ex);
        server.run().await.unwrap();

        // An idle connection takes the only slot.
        let idle = StdTcpStream::connect(("127.0.0.1", port)).unwrap();
        sleep(Duration::from_millis(100)).await;

        let client = StunClient::new(endpoint);
        assert!(client.discover().await.is_err());

        // Closing it frees the slot.
        drop(idle);
        sleep(Duration::from_millis(100)).await;
        assert!(client.discover().await.is_ok());

        server.shutdown().await;
    });
}

#[cfg(feature = "tls")]
#[test]
fn tls_binding() {
    run_test_with_executor(15, |ex| async move {
        let _ = default_provider().install_default();
        let (certs, key) = test_certs();

        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs.clone(), key.clone_key())
            .unwrap();
        let mut roots = RootCertStore::empty();
        for cert in &certs {
            roots.add(cert.clone()).unwrap();
        }
        let client_config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();

        let port = free_tcp_port();
        let endpoint: Endpoint = format!("tls://127.0.0.1:{port}").parse().unwrap();

        let config = StunConfig {
            tls_endpoint: Some((endpoint.clone(), ServerTlsConfig { server_config })),
            ..Default::default()
        };
        let server = StunServer::new(config, ex);
        server.run().await.unwrap();

        let client = StunClient::new(endpoint).with_tls(ClientTlsConfig {
            client_config,
            dns_name: "localhost".into(),
        });
        let addr = client.discover().await.unwrap();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);

        server.shutdown().await;
    });
}

#[cfg(feature = "quic")]
#[test]
fn quic_binding() {
    run_test_with_executor(15, |ex| async move {
        let (certs, key) = test_certs();

        let port = free_udp_port();
        let endpoint: Endpoint = format!("quic://127.0.0.1:{port}").parse().unwrap();

        let config = StunConfig {
            quic_endpoint: Some((endpoint.clone(), ServerQuicConfig::new(certs.clone(), key))),
            ..Default::default()
        };
        let server = StunServer::new(config, ex);
        server.run().await.unwrap();

        let client = StunClient::new(endpoint).with_quic(ClientQuicConfig::new(certs, "localhost"));
        let addr = client.discover().await.unwrap();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);

        server.shutdown().await;
    });
}
