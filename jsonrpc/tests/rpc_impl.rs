use karyon_jsonrpc::{rpc_impl, Error, RPCService};
use serde_json::Value;

#[test]
fn rpc_impl_service() {
    struct Foo {}

    #[rpc_impl]
    impl Foo {
        async fn foo(&self, params: Value) -> Result<Value, Error> {
            Ok(params)
        }
    }

    let f = Foo {};

    assert!(f.get_method("foo").is_some());
    assert!(f.get_method("bar").is_none());

    let params = serde_json::json!("params");

    smol::block_on(async {
        let foo_method = f.get_method("foo").unwrap();
        assert_eq!(foo_method(params.clone()).await.unwrap(), params);
    });
}
