mod shared;

use std::sync::Arc;

use serde_json::Value;

use karyon_core::testing::run_test;

use karyon_jsonrpc::{client::ClientBuilder, server::ServerBuilder};

use shared::MathService;

fn unix_endpoint() -> String {
    let id = karyon_core::util::random_32();
    let dir = std::env::temp_dir();
    let path = dir.join(format!("karyon_test_{id}.socket"));
    format!("unix:{}", path.display())
}

#[test]
fn rpc_call() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let ep = unix_endpoint();

        let server = ServerBuilder::new(ep.as_str())
            .expect("Create server builder")
            .service(service)
            .build()
            .await
            .expect("Build server");

        server.clone().start();

        let client = ClientBuilder::new(ep.as_str())
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
        let ep = unix_endpoint();

        let server = ServerBuilder::new(ep.as_str())
            .expect("Create server builder")
            .service(service)
            .build()
            .await
            .expect("Build server");

        server.clone().start();

        let client = ClientBuilder::new(ep.as_str())
            .expect("Create client builder")
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
        let ep = unix_endpoint();

        let server = ServerBuilder::new(ep.as_str())
            .expect("Create server builder")
            .service(service.clone())
            .pubsub_service(service)
            .build()
            .await
            .expect("Build server");

        server.clone().start();

        let client = ClientBuilder::new(ep.as_str())
            .expect("Create client builder")
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
