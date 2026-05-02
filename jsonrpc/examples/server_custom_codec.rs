use std::{io, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_jsonrpc::{
    codec::{ByteBuffer, Codec},
    error::RPCError,
    rpc_impl,
    server::ServerBuilder,
};
use karyon_net::Error as NetError;

struct Calc {}

#[derive(Deserialize, Serialize)]
struct Pong {}

#[rpc_impl]
impl Calc {
    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!(Pong {}))
    }
}

/// A minimal custom JSON codec, identical in wire format to the
/// built-in `JsonCodec` but written out for illustration.
#[derive(Clone)]
pub struct CustomJsonCodec {}

impl Codec<ByteBuffer> for CustomJsonCodec {
    type Message = serde_json::Value;
    type Error = NetError;

    fn encode(&self, src: &serde_json::Value, dst: &mut ByteBuffer) -> Result<usize, NetError> {
        let bytes = serde_json::to_vec(src).map_err(|e| NetError::IO(io::Error::other(e)))?;
        let n = bytes.len();
        dst.extend_from_slice(&bytes);
        Ok(n)
    }

    fn decode(&self, src: &mut ByteBuffer) -> Result<Option<(usize, serde_json::Value)>, NetError> {
        let de = serde_json::Deserializer::from_slice(src.as_ref());
        let mut iter = de.into_iter::<serde_json::Value>();

        let item = match iter.next() {
            Some(Ok(item)) => item,
            Some(Err(ref e)) if e.is_eof() => return Ok(None),
            Some(Err(e)) => return Err(NetError::IO(io::Error::other(e))),
            None => return Ok(None),
        };

        Ok(Some((iter.byte_offset(), item)))
    }
}

fn main() {
    env_logger::init();
    smol::block_on(async {
        let calc = Calc {};

        let server = ServerBuilder::new_with_codec("tcp://127.0.0.1:6000", CustomJsonCodec {})
            .expect("Create a new server builder")
            .service(Arc::new(calc))
            .build()
            .await
            .expect("start a new server");

        server.start_block().await.expect("Start the server");
    });
}
