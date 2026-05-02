#![cfg(feature = "http3")]

mod shared;

use std::sync::Arc;

use serde_json::Value;

use karyon_core::testing::run_test;

use karyon_jsonrpc::{client::ClientBuilder, server::ServerBuilder};
use karyon_net::quic::{ClientQuicConfig, ServerQuicConfig};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use shared::{generate_test_certs, MathService};

fn test_quic_configs() -> (ServerQuicConfig, ClientQuicConfig) {
    let (cert_chain, private_key): (Vec<CertificateDer>, std::sync::Arc<PrivateKeyDer>) =
        generate_test_certs();
    let server = ServerQuicConfig::new(cert_chain.clone(), private_key);
    let client = ClientQuicConfig::new(cert_chain, "localhost");
    (server, client)
}

#[test]
fn rpc_call() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .quic_config(cli_cfg)
            .expect("Set quic config")
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
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .quic_config(cli_cfg)
            .expect("Set quic config")
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
fn multiple_clients() {
    run_test(15, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let mut clients = Vec::new();
        for _ in 0..3 {
            let client = ClientBuilder::new(endpoint.to_string())
                .expect("Create client builder")
                .quic_config(cli_cfg.clone())
                .expect("Set quic config")
                .build()
                .await
                .expect("Build client");
            clients.push(client);
        }

        for (i, client) in clients.iter().enumerate() {
            let x = i as i32;
            let result: i32 = client
                .call("MathService.add", serde_json::json!({"x": x, "y": x}))
                .await
                .expect("Call add");
            assert_eq!(result, x * 2);
        }

        for client in &clients {
            client.stop().await;
        }
        server.shutdown().await;
    });
}

#[test]
fn pubsub_subscription() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service.clone())
            .pubsub_service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .quic_config(cli_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build client");

        let sub = client
            .subscribe("MathService.counter_subscribe", ())
            .await
            .expect("Subscribe");

        let msg1: i32 = serde_json::from_value(sub.recv().await.expect("Recv notification 1"))
            .expect("Parse notification");
        let msg2: i32 = serde_json::from_value(sub.recv().await.expect("Recv notification 2"))
            .expect("Parse notification");

        assert_eq!(msg1, 1);
        assert_eq!(msg2, 2);

        client
            .unsubscribe("MathService.counter_unsubscribe", sub.id())
            .await
            .expect("Unsubscribe");

        client.stop().await;
        server.shutdown().await;
    });
}
