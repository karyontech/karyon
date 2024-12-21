use std::sync::Arc;

use karyon_jsonrpc::{
    error::RPCError,
    rpc_pubsub_impl,
    server::{Channel, PubSubRPCService},
};
use serde_json::Value;

#[test]
fn rpc_pubsub_impl_service() {
    struct Foo {}

    #[rpc_pubsub_impl]
    impl Foo {
        async fn foo(
            &self,
            _channel: Arc<Channel>,
            _method: String,
            params: Value,
        ) -> Result<Value, RPCError> {
            Ok(params)
        }
    }

    let f = Arc::new(Foo {});

    assert!(f.get_pubsub_method("foo").is_some());
    assert!(f.get_pubsub_method("bar").is_none());

    let _params = serde_json::json!("params");

    // TODO add more tests here
}
