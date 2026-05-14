//! Integration tests for rpc_impl / rpc_method / rpc_pubsub_impl macros.
//! Runs over TCP; covers default and renamed service/method names.

use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_core::{async_util::sleep, testing::run_test, util::random_32};

use karyon_jsonrpc::{
    client::ClientBuilder,
    error::RPCError,
    message::SubscriptionID,
    rpc_impl, rpc_method, rpc_pubsub_impl,
    server::{channel::Channel, ServerBuilder},
};

// Service with default (struct-name-derived) name and mix of
// default + renamed methods.
struct DefaultNamed {}

#[derive(Deserialize, Serialize)]
struct Pair {
    x: i32,
    y: i32,
}

#[rpc_impl]
impl DefaultNamed {
    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!("pong"))
    }

    #[rpc_method(name = "math.add")]
    async fn add(&self, params: Value) -> Result<Value, RPCError> {
        let p: Pair = serde_json::from_value(params)?;
        Ok(serde_json::json!(p.x + p.y))
    }

    #[rpc_method(name = "math.sub")]
    async fn sub(&self, params: Value) -> Result<Value, RPCError> {
        let p: Pair = serde_json::from_value(params)?;
        Ok(serde_json::json!(p.x - p.y))
    }
}

// Service with a custom name.
struct Calc {}

#[rpc_impl(name = "calculator")]
impl Calc {
    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!("pong"))
    }

    #[rpc_method(name = "math.mul")]
    async fn mul(&self, params: Value) -> Result<Value, RPCError> {
        let p: Pair = serde_json::from_value(params)?;
        Ok(serde_json::json!(p.x * p.y))
    }
}

// Pubsub service with default name + renamed method.
struct PubSubDefault {}

#[rpc_impl]
impl PubSubDefault {
    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!("pong"))
    }
}

#[rpc_pubsub_impl]
impl PubSubDefault {
    #[rpc_method(name = "counter.sub")]
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
        let sub_id = sub.id();
        karyon_core::async_runtime::spawn(async move {
            let mut count = 0;
            loop {
                sleep(Duration::from_millis(30)).await;
                count += 1;
                if sub.notify(serde_json::json!(count)).await.is_err() {
                    break;
                }
            }
        })
        .detach();
        Ok(serde_json::json!(sub_id))
    }

    #[rpc_method(name = "counter.unsub")]
    async fn counter_unsubscribe(
        &self,
        chan: Arc<Channel>,
        _method: String,
        params: Value,
    ) -> Result<Value, RPCError> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        let ok = chan.remove_subscription(&sub_id).await.is_ok();
        Ok(serde_json::json!(ok))
    }
}

#[test]
fn rpc_impl_default_service_name() {
    run_test(10, async {
        let server = ServerBuilder::new("tcp://127.0.0.1:0")
            .expect("builder")
            .service(Arc::new(DefaultNamed {}))
            .build()
            .await
            .expect("build");
        let ep = server.local_endpoint().expect("ep");
        server.clone().start();

        let client = ClientBuilder::new(ep.to_string())
            .expect("builder")
            .build()
            .await
            .expect("build");

        let r: String = client.call("DefaultNamed.ping", ()).await.expect("ping");
        assert_eq!(r, "pong");

        let r: i32 = client
            .call("DefaultNamed.math.add", serde_json::json!({"x": 7, "y": 3}))
            .await
            .expect("add");
        assert_eq!(r, 10);

        let r: i32 = client
            .call("DefaultNamed.math.sub", serde_json::json!({"x": 7, "y": 3}))
            .await
            .expect("sub");
        assert_eq!(r, 4);

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn rpc_impl_custom_service_name() {
    run_test(10, async {
        let server = ServerBuilder::new("tcp://127.0.0.1:0")
            .expect("builder")
            .service(Arc::new(Calc {}))
            .build()
            .await
            .expect("build");
        let ep = server.local_endpoint().expect("ep");
        server.clone().start();

        let client = ClientBuilder::new(ep.to_string())
            .expect("builder")
            .build()
            .await
            .expect("build");

        let r: String = client.call("calculator.ping", ()).await.expect("ping");
        assert_eq!(r, "pong");

        let r: i32 = client
            .call("calculator.math.mul", serde_json::json!({"x": 6, "y": 7}))
            .await
            .expect("mul");
        assert_eq!(r, 42);

        let err: Result<Value, _> = client.call("Calc.ping", ()).await;
        assert!(err.is_err(), "default name should not be registered");

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn rpc_pubsub_impl_renamed_methods() {
    run_test(10, async {
        let service = Arc::new(PubSubDefault {});
        let server = ServerBuilder::new("tcp://127.0.0.1:0")
            .expect("builder")
            .service(service.clone())
            .pubsub_service(service)
            .build()
            .await
            .expect("build");
        let ep = server.local_endpoint().expect("ep");
        server.clone().start();

        let client = ClientBuilder::new(ep.to_string())
            .expect("builder")
            .build()
            .await
            .expect("build");

        let sub = client
            .subscribe("PubSubDefault.counter.sub", ())
            .await
            .expect("subscribe");

        let v1: i32 = serde_json::from_value(sub.recv().await.expect("nt1")).expect("parse");
        assert_eq!(v1, 1);

        client
            .unsubscribe("PubSubDefault.counter.unsub", sub.id())
            .await
            .expect("unsubscribe");

        client.stop().await;
        server.shutdown().await;
    });
}
