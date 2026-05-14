use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_core::{async_util::sleep, util::random_32};

use karyon_jsonrpc::{
    error::RPCError, message::SubscriptionID, rpc_impl, rpc_pubsub_impl, server::channel::Channel,
};

pub struct MathService {}

#[derive(Deserialize, Serialize)]
pub struct AddParams {
    pub x: i32,
    pub y: i32,
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
        let sub_id = sub.id();
        karyon_core::async_runtime::spawn(async move {
            let mut count = 0;
            loop {
                sleep(Duration::from_millis(50)).await;
                count += 1;
                if sub.notify(serde_json::json!(count)).await.is_err() {
                    break;
                }
            }
        })
        .detach();

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

/// Generate a self-signed certificate and key for testing.
#[allow(dead_code)]
pub fn generate_test_certs() -> (
    Vec<rustls_pki_types::CertificateDer<'static>>,
    Arc<rustls_pki_types::PrivateKeyDer<'static>>,
) {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_der = rustls_pki_types::CertificateDer::from(certified.cert);
    let key_der: rustls_pki_types::PrivateKeyDer<'static> = certified.signing_key.into();
    (vec![cert_der], Arc::new(key_der))
}
