# karyon jsonrpc macro

Procedural macros that turn Rust `impl` blocks into JSON-RPC 2.0 services for
[karyon_jsonrpc](https://crates.io/crates/karyon_jsonrpc).

- `#[rpc_impl]` implements `RPCService` for the type of an `impl` block.
- `#[rpc_pubsub_impl]` implements `PubSubRPCService` for the type of an `impl` block.
- `#[rpc_method(name = "...")]` renames a method inside either block.

The service name defaults to the type name. Set another with `#[rpc_impl(name = "...")]` or
`#[rpc_pubsub_impl(name = "...")]`.

Do not depend on this crate directly. Use karyon_jsonrpc, which re-exports the macros.
