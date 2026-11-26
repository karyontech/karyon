#[path = "shared/positional.rs"]
mod positional;
mod shared;

use std::{sync::Arc, time::Duration};

use serde_json::{Value, json};

use karyon_core::{
    async_runtime::io::{AsyncReadExt, AsyncWriteExt},
    async_util::timeout,
    testing::run_test,
};

use karyon_jsonrpc::{
    client::ClientBuilder, codec::JsonCodec, error::Error, server::ServerBuilder,
};
use karyon_net::{
    StreamMux,
    quic::{ClientQuicConfig, QuicEndpoint, ServerQuicConfig},
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use shared::{MathService, generate_test_certs};

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

        let server = ServerBuilder::new("quic://127.0.0.1:0")
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

        let server = ServerBuilder::new("quic://127.0.0.1:0")
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

        let server = ServerBuilder::new("quic://127.0.0.1:0")
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

        let server = ServerBuilder::new("quic://127.0.0.1:0")
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

/// A failed call returns `CallError`.
#[test]
fn call_returns_call_error() {
    run_test(10, async {
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("quic://127.0.0.1:0")
            .expect("Create server builder")
            .service(Arc::new(MathService {}))
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
        assert!(
            matches!(result, Err(Error::CallError(500, _))),
            "Should return a call error: {result:?}"
        );

        client.stop().await;
        server.shutdown().await;
    });
}

/// A server with a custom notification format and a client with the
/// matching decoder.
#[test]
fn custom_notification_format() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("quic://127.0.0.1:0")
            .expect("Create server builder")
            .pubsub_service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .with_notification_encoder(positional::encode)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .quic_config(cli_cfg)
            .expect("Set quic config")
            .with_notification_decoder(positional::decode)
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

        client.stop().await;
        server.shutdown().await;
    });
}

/// When the server ends the subscription stream, `recv()` returns an
/// error instead of waiting forever.
#[test]
fn subscription_closes_when_stream_ends() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("quic://127.0.0.1:0")
            .expect("Create server builder")
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
        sub.recv().await.expect("Recv notification");

        server.shutdown().await;

        // Buffered notifications may come first, then `recv()` fails.
        let closed = timeout(Duration::from_secs(2), async {
            while sub.recv().await.is_ok() {}
        })
        .await;
        assert!(closed.is_ok(), "subscription did not close");

        client.stop().await;
    });
}

/// The server codec and its size limit apply to QUIC streams.
#[test]
fn codec_max_size_applies() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new_with_codec("quic://127.0.0.1:0", JsonCodec::new(256))
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

        let small: String = client
            .call("MathService.echo", json!("hello"))
            .await
            .expect("Call echo");
        assert_eq!(small, "hello");

        let big = "x".repeat(1024);
        let res: Result<String, Error> = client.call("MathService.echo", json!(big)).await;
        assert!(res.is_err(), "a request over the codec limit was accepted");

        client.stop().await;
        server.shutdown().await;
    });
}

/// A stream that never sends a whole request is closed after
/// `read_timeout`.
#[test]
fn idle_stream_closed_after_read_timeout() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("quic://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .read_timeout(Duration::from_millis(200))
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let conn = QuicEndpoint::dial(&endpoint, cli_cfg).await.expect("Dial");
        let mut stream = conn.open_stream().await.expect("Open stream");
        // QUIC shows a stream to the peer only once it carries data, so
        // send the start of a request and never finish it.
        stream.write_all(b"{").await.expect("Write");
        stream.flush().await.expect("Flush");

        let mut buf = [0u8; 64];
        let closed = timeout(Duration::from_secs(2), stream.read(&mut buf)).await;
        assert!(closed.is_ok(), "idle stream was not closed");

        server.shutdown().await;
    });
}

#[test]
fn call_and_subscribe_timeout() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("quic://127.0.0.1:0")
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
            .set_timeout(100)
            .build()
            .await
            .expect("Build client");

        let call_res: Result<Value, _> = client.call("MathService.hang", ()).await;
        assert!(
            matches!(
                call_res,
                Err(Error::KaryonCore(karyon_core::error::Error::Timeout))
            ),
            "Expected timeout error on call, got: {call_res:?}"
        );

        let sub_res = client.subscribe("MathService.hang_subscribe", ()).await;
        assert!(
            matches!(
                sub_res,
                Err(Error::KaryonCore(karyon_core::error::Error::Timeout))
            ),
            "Expected timeout error on subscribe, got: {:?}",
            sub_res.err()
        );

        client.stop().await;
        server.shutdown().await;
    });
}
