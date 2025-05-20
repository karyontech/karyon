use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use karyon_jsonrpc::{
    codec::{ByteBuffer, Codec, Decoder, Encoder},
    error::{Error, RPCError},
    rpc_impl,
    server::ServerBuilder,
};

struct Calc {}

#[derive(Deserialize, Serialize)]
struct Req {
    x: u32,
    y: u32,
}

#[derive(Deserialize, Serialize)]
struct Pong {}

#[rpc_impl]
impl Calc {
    async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
        Ok(serde_json::json!(Pong {}))
    }
}

#[derive(Clone)]
pub struct CustomJsonCodec {}

impl Codec for CustomJsonCodec {
    type Message = serde_json::Value;
    type Error = Error;
}

impl Encoder for CustomJsonCodec {
    type EnMessage = serde_json::Value;
    type EnError = Error;
    fn encode(
        &self,
        src: &Self::EnMessage,
        dst: &mut ByteBuffer,
    ) -> std::result::Result<usize, Self::EnError> {
        let msg = match serde_json::to_string(src) {
            Ok(m) => m,
            Err(err) => return Err(Error::Encode(err.to_string())),
        };
        let buf = msg.as_bytes();
        dst.extend_from_slice(buf);
        Ok(buf.len())
    }
}

impl Decoder for CustomJsonCodec {
    type DeMessage = serde_json::Value;
    type DeError = Error;
    fn decode(
        &self,
        src: &mut ByteBuffer,
    ) -> std::result::Result<Option<(usize, Self::DeMessage)>, Self::DeError> {
        let de = serde_json::Deserializer::from_slice(src.as_ref());
        let mut iter = de.into_iter::<serde_json::Value>();

        let item = match iter.next() {
            Some(Ok(item)) => item,
            Some(Err(ref e)) if e.is_eof() => return Ok(None),
            Some(Err(e)) => return Err(Error::Decode(e.to_string())),
            None => return Ok(None),
        };

        Ok(Some((iter.byte_offset(), item)))
    }
}

fn main() {
    env_logger::init();
    smol::block_on(async {
        // Register the Calc service
        let calc = Calc {};

        // Creates a new server
        let server = ServerBuilder::new_with_codec("tcp://127.0.0.1:6000", CustomJsonCodec {})
            .expect("Create a new server builder")
            .service(Arc::new(calc))
            .build()
            .await
            .expect("start a new server");

        // Start the server
        server.start_block().await.expect("Start the server");
    });
}
