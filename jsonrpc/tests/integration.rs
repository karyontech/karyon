use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_core::{
    async_util::{sleep, timeout},
    util::random_32,
};

use karyon_jsonrpc::{
    client::ClientBuilder,
    error::RPCError,
    message::SubscriptionID,
    rpc_impl, rpc_pubsub_impl,
    server::{channel::Channel, ServerBuilder},
};

struct MathService {}

#[derive(Deserialize, Serialize)]
struct AddParams {
    x: i32,
    y: i32,
}

#[rpc_impl]
impl MathService {
    async fn add(&self, params: Value) -> Result<Value, RPCError> {
        let params: AddParams = serde_json::from_value(params)?;
        Ok(serde_json::json!(params.x + params.y))
    }

    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!("pong"))
    }

    async fn error_method(&self, _params: Value) -> Result<Value, RPCError> {
        Err(RPCError::CustomError(500, "something went wrong".into()))
    }
}

#[rpc_pubsub_impl]
impl MathService {
    async fn counter_subscribe(
        &self,
        chan: Arc<Channel>,
        method: String,
        _params: Value,
    ) -> Result<Value, RPCError> {
        let sub = chan
            .new_subscription(&method, Some(random_32()))
            .await
            .map_err(|_| RPCError::InvalidRequest("Duplicated subscription".into()))?;
        let sub_id = sub.id;
        let task = karyon_core::async_runtime::spawn(async move {
            let mut count = 0;
            loop {
                sleep(Duration::from_millis(50)).await;
                count += 1;
                if sub.notify(serde_json::json!(count)).await.is_err() {
                    break;
                }
            }
        });
        // Prevent the task from being cancelled when dropped.
        std::mem::forget(task);
        Ok(serde_json::json!(sub_id))
    }

    async fn counter_unsubscribe(
        &self,
        chan: Arc<Channel>,
        _method: String,
        params: Value,
    ) -> Result<Value, RPCError> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        let success = chan.remove_subscription(&sub_id).await.is_ok();
        Ok(serde_json::json!(success))
    }
}

/// Run an async test with a timeout.
/// Uses the global executor so that spawned tasks (e.g. pubsub notifications)
/// are properly driven.
fn run_test<F: std::future::Future<Output = ()> + Send + 'static>(timeout_secs: u64, f: F) {
    let _ = env_logger::try_init();
    #[cfg(feature = "smol")]
    smol::block_on(smol::spawn(async move {
        timeout(Duration::from_secs(timeout_secs), f)
            .await
            .unwrap_or_else(|_| panic!("Test timed out after {timeout_secs}s"));
    }));

    #[cfg(feature = "tokio")]
    {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        rt.block_on(async {
            timeout(Duration::from_secs(timeout_secs), f)
                .await
                .unwrap_or_else(|_| panic!("Test timed out after {timeout_secs}s"));
        });
    }
}

/// Test: Basic RPC call and response over TCP.
#[test]
fn rpc_call_over_tcp() {
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

/// Test: RPC error handling.
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

        // Call a method that returns an error
        let result: Result<Value, _> = client.call("MathService.error_method", ()).await;
        assert!(result.is_err(), "Should return an error");

        // Call a method that doesn't exist
        let result: Result<Value, _> = client.call("MathService.nonexistent", ()).await;
        assert!(result.is_err(), "Nonexistent method should return error");

        // Call a service that doesn't exist
        let result: Result<Value, _> = client.call("Unknown.method", ()).await;
        assert!(result.is_err(), "Unknown service should return error");

        client.stop().await;
        server.shutdown().await;
    });
}

/// Test: Multiple clients connecting to the same server.
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

/// Test: PubSub subscription and notification receiving.
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

/// Test: RPC call still works alongside active subscriptions.
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

/// Test: RPC call over WebSocket.
#[cfg(feature = "ws")]
#[test]
fn rpc_call_over_websocket() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new("ws://127.0.0.1:0")
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

        // Test RPC
        let result: String = client
            .call("MathService.ping", ())
            .await
            .expect("Call ping over WS");
        assert_eq!(result, "pong");

        let result: i32 = client
            .call("MathService.add", serde_json::json!({"x": 7, "y": 8}))
            .await
            .expect("Call add over WS");
        assert_eq!(result, 15);

        // Test PubSub over WS
        let sub = client
            .subscribe("MathService.counter_subscribe", ())
            .await
            .expect("Subscribe over WS");

        let msg: i32 = serde_json::from_value(sub.recv().await.expect("Recv notification"))
            .expect("Parse notification");
        assert!(msg >= 1);

        client
            .unsubscribe("MathService.counter_unsubscribe", sub.id())
            .await
            .expect("Unsubscribe over WS");

        client.stop().await;
        server.shutdown().await;
    });
}
