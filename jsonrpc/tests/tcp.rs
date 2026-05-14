mod shared;

use std::sync::Arc;

use serde_json::Value;

use karyon_core::testing::run_test;

use karyon_jsonrpc::{client::ClientBuilder, server::ServerBuilder};

use shared::MathService;

#[test]
fn rpc_call() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new("tcp://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
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

        let server = ServerBuilder::new("tcp://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .build()
            .await
            .expect("Build client");

        let result: Result<Value, _> = client.call("MathService.error_method", ()).await;
        assert!(result.is_err(), "Should return an error");

        let result: Result<Value, _> = client.call("MathService.nonexistent", ()).await;
        assert!(result.is_err(), "Nonexistent method should return error");

        let result: Result<Value, _> = client.call("Unknown.method", ()).await;
        assert!(result.is_err(), "Unknown service should return error");

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn multiple_clients() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new("tcp://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let mut clients = Vec::new();
        for _ in 0..3 {
            let client = ClientBuilder::new(endpoint.to_string())
                .expect("Create client builder")
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

        let server = ServerBuilder::new("tcp://127.0.0.1:0")
            .expect("Create server builder")
            .service(service.clone())
            .pubsub_service(service)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .build()
            .await
            .expect("Build client");

        let sub = client
            .subscribe("MathService.counter_subscribe", ())
            .await
            .expect("Subscribe to counter");

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

#[test]
fn rpc_and_pubsub_together() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new("tcp://127.0.0.1:0")
            .expect("Create server builder")
            .service(service.clone())
            .pubsub_service(service)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .build()
            .await
            .expect("Build client");

        let sub = client
            .subscribe("MathService.counter_subscribe", ())
            .await
            .expect("Subscribe");

        let result: String = client
            .call("MathService.ping", ())
            .await
            .expect("Call ping");
        assert_eq!(result, "pong");

        let result: i32 = client
            .call("MathService.add", serde_json::json!({"x": 5, "y": 3}))
            .await
            .expect("Call add");
        assert_eq!(result, 8);

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
