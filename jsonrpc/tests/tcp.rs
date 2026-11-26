#[path = "shared/positional.rs"]
mod positional;
mod shared;

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde_json::Value;

use karyon_core::{
    async_runtime::{lock::Semaphore, spawn},
    async_util::sleep,
    testing::run_test,
};

use karyon_jsonrpc::{client::ClientBuilder, error::Error, server::ServerBuilder};

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
        assert!(
            matches!(result, Err(Error::CallError(500, _))),
            "Should return a call error: {result:?}"
        );

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

/// A server with a custom notification format and a client with the
/// matching decoder.
#[test]
fn custom_notification_format() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new("tcp://127.0.0.1:0")
            .expect("Create server builder")
            .pubsub_service(service)
            .with_notification_encoder(positional::encode)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
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

#[test]
fn max_concurrent_requests_bounds_in_flight_requests() {
    run_test(10, async {
        let gate = Arc::new(Semaphore::new(1));
        let hold = gate.acquire().await.expect("acquire gate");

        let started = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));

        let server = ServerBuilder::new("tcp://127.0.0.1:0")
            .expect("Create server builder")
            .max_concurrent_requests(2)
            .method("slow", {
                let gate = gate.clone();
                let started = started.clone();
                let active = active.clone();
                let max_active = max_active.clone();
                move |params| {
                    let gate = gate.clone();
                    let started = started.clone();
                    let active = active.clone();
                    let max_active = max_active.clone();
                    async move {
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        max_active.fetch_max(now, Ordering::SeqCst);
                        started.fetch_add(1, Ordering::SeqCst);
                        let _permit = gate.acquire().await.expect("acquire gate");
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(params)
                    }
                }
            })
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

        let mut tasks = Vec::new();
        for i in 0..4usize {
            let client = client.clone();
            tasks.push(spawn(
                async move { client.call::<_, usize>("slow", i).await },
            ));
        }

        while started.load(Ordering::SeqCst) < 2 {
            sleep(Duration::from_millis(5)).await;
        }
        sleep(Duration::from_millis(30)).await;
        assert_eq!(started.load(Ordering::SeqCst), 2);
        assert_eq!(max_active.load(Ordering::SeqCst), 2);

        drop(hold);

        for (i, task) in tasks.into_iter().enumerate() {
            let res = task
                .await
                .expect("join rpc call task")
                .expect("slow rpc succeeds");
            assert_eq!(res, i);
        }
        assert_eq!(max_active.load(Ordering::SeqCst), 2);

        client.stop().await;
        server.shutdown().await;
    });
}
