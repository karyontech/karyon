mod shared;

use std::{net::SocketAddr, sync::Arc};

use serde_json::Value;

use karyon_core::{async_runtime::net::TcpStream, testing::run_test};

use karyon_jsonrpc::{client::ClientBuilder, server::ServerBuilder};
use karyon_net::tls::{ClientTlsConfig, ServerTlsConfig};

use shared::{generate_test_certs, MathService};

fn test_tls_configs() -> (ServerTlsConfig, ClientTlsConfig) {
    let _ =
        karyon_net::async_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
    let (cert_chain, private_key) = generate_test_certs();

    let server_config = karyon_net::async_rustls::rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain.clone(), private_key.clone_key())
        .expect("Build server TLS config");

    let mut root_store = karyon_net::async_rustls::rustls::RootCertStore::empty();
    for cert in &cert_chain {
        root_store.add(cert.clone()).expect("Add root cert");
    }
    let client_config = karyon_net::async_rustls::rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    (
        ServerTlsConfig { server_config },
        ClientTlsConfig {
            client_config,
            dns_name: "localhost".to_string(),
        },
    )
}

#[test]
fn rpc_call() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (server_tls, client_tls) = test_tls_configs();

        let server = ServerBuilder::new("tls://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .tls_config(server_tls)
            .expect("Set TLS config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .tls_config(client_tls)
            .expect("Set TLS config")
            .build()
            .await
            .expect("Build client");

        let result: String = client
            .call("MathService.ping", ())
            .await
            .expect("Call ping");
        assert_eq!(result, "pong");

        let result: i32 = client
            .call("MathService.add", serde_json::json!({"x": 10, "y": 20}))
            .await
            .expect("Call add");
        assert_eq!(result, 30);

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn rpc_error_handling() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (server_tls, client_tls) = test_tls_configs();

        let server = ServerBuilder::new("tls://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .tls_config(server_tls)
            .expect("Set TLS config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .tls_config(client_tls)
            .expect("Set TLS config")
            .build()
            .await
            .expect("Build client");

        let result: Result<Value, _> = client.call("MathService.error_method", ()).await;
        assert!(result.is_err());

        let result: Result<Value, _> = client.call("MathService.nonexistent", ()).await;
        assert!(result.is_err());

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn pubsub_subscription() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (server_tls, client_tls) = test_tls_configs();

        let server = ServerBuilder::new("tls://127.0.0.1:0")
            .expect("Create server builder")
            .service(service.clone())
            .pubsub_service(service)
            .tls_config(server_tls)
            .expect("Set TLS config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .tls_config(client_tls)
            .expect("Set TLS config")
            .build()
            .await
            .expect("Build client");

        let sub = client
            .subscribe("MathService.counter_subscribe", ())
            .await
            .expect("Subscribe");

        let msg: i32 = serde_json::from_value(sub.recv().await.expect("Recv notification"))
            .expect("Parse notification");
        assert!(msg >= 1);

        client
            .unsubscribe("MathService.counter_unsubscribe", sub.id())
            .await
            .expect("Unsubscribe");

        client.stop().await;
        server.shutdown().await;
    });
}

/// Clients that open a TCP connection and never start the TLS
/// handshake must not stall the accept loop.
#[test]
fn stalled_handshake_does_not_block_accept() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (server_tls, client_tls) = test_tls_configs();

        let server = ServerBuilder::new("tls://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .tls_config(server_tls)
            .expect("Set TLS config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        // Connect but never send a ClientHello.
        let addr = SocketAddr::try_from(endpoint.clone()).expect("Endpoint to socket addr");
        let mut stalled = Vec::new();
        for _ in 0..8 {
            stalled.push(TcpStream::connect(addr).await.expect("Open raw connection"));
        }

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .tls_config(client_tls)
            .expect("Set TLS config")
            .build()
            .await
            .expect("Build client");

        let result: String = client
            .call("MathService.ping", ())
            .await
            .expect("Call ping");
        assert_eq!(result, "pong");

        drop(stalled);
        client.stop().await;
        server.shutdown().await;
    });
}
