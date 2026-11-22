//! A JSON-RPC server and client on a runtime and a transport you supply,
//! as on WASI: every karyon task runs on the main thread, the timers are
//! yours, and the client reaches the server through in-memory pipes
//! instead of OS sockets. See `support/one_thread.rs` and
//! `support/mem_transport.rs`.
//!
//! ```text
//! cargo run -p karyon_jsonrpc --example custom_runtime
//!
//! # on WASI
//! CARGO_TARGET_WASM32_WASIP2_RUNNER=wasmtime cargo run -p karyon_jsonrpc \
//!     --example custom_runtime --target wasm32-wasip2 \
//!     --no-default-features --features tokio
//! ```

#[path = "support/calc.rs"]
mod calc;
#[path = "support/mem_transport.rs"]
mod mem_transport;
#[path = "support/one_thread.rs"]
mod one_thread;

use one_thread::run;

fn main() {
    let sum = run(|ex| calc::add(ex, 2, 40));
    println!("Calc.add(2, 40) = {sum}");
}
