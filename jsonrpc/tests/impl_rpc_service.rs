use karyon_jsonrpc::{impl_rpc_service, RPCError, RPCService};
use serde_json::Value;

#[test]
fn service() {
    struct Foo {}

    impl Foo {
        async fn foo(&self, params: Value) -> Result<Value, RPCError> {
            Ok(params)
        }
    }

    impl_rpc_service!(Foo, foo);

    let f = Foo {};

    assert!(f.get_method("foo").is_some());
    assert!(f.get_method("bar").is_none());

    let params = serde_json::json!("params");

    smol::block_on(async {
        let foo_method = f.get_method("foo").unwrap();
        assert_eq!(foo_method(params.clone()).await.unwrap(), params);
    });
}
