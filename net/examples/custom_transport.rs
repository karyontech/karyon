//! A custom transport: `mem://` endpoints over in-memory pipes instead of
//! OS sockets, framed with `LengthCodec`. It runs natively and on WASI,
//! where karyon_net has no sockets of its own. See
//! `support/mem_transport.rs`.
//!
//! ```text
//! cargo run -p karyon_net --example custom_transport
//!
//! # on WASI
//! CARGO_TARGET_WASM32_WASIP2_RUNNER=wasmtime cargo run -p karyon_net \
//!     --example custom_transport --target wasm32-wasip2 \
//!     --no-default-features --features tokio,framing
//! ```

#[path = "support/mem_transport.rs"]
mod mem_transport;

use karyon_net::{Endpoint, Result, codec::LengthCodec, framed, transport::Transport};

use mem_transport::MemTransport;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let transport = MemTransport::new();
    let endpoint: Endpoint = "mem://server".parse()?;

    let listener = transport.listen(&endpoint).await?;
    let client = transport.dial(&endpoint).await?;
    let server = listener.accept().await?;

    let mut client = framed(client, LengthCodec::default());
    let mut server = framed(server, LengthCodec::default());

    client.send_msg(b"ping".to_vec()).await?;
    let msg = server.recv_msg().await?;
    println!("server got {:?}", String::from_utf8_lossy(&msg));

    server.send_msg(b"pong".to_vec()).await?;
    let msg = client.recv_msg().await?;
    println!("client got {:?}", String::from_utf8_lossy(&msg));
    Ok(())
}
