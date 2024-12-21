use std::time::Duration;

use log::info;
use serde::{Deserialize, Serialize};
use smol::Timer;

use karyon_jsonrpc::{
    client::ClientBuilder,
    codec::{Codec, Decoder, Encoder},
    error::Error,
};

#[derive(Deserialize, Serialize)]
struct Req {
    x: u32,
    y: u32,
}

#[derive(Deserialize, Serialize, Debug)]
struct Pong {}

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
        dst: &mut [u8],
    ) -> std::result::Result<usize, Self::EnError> {
        let msg = match serde_json::to_string(src) {
            Ok(m) => m,
            Err(err) => return Err(Error::Encode(err.to_string())),
        };
        let buf = msg.as_bytes();
        dst[..buf.len()].copy_from_slice(buf);
        Ok(buf.len())
    }
}

impl Decoder for CustomJsonCodec {
    type DeMessage = serde_json::Value;
    type DeError = Error;
    fn decode(
        &self,
        src: &mut [u8],
    ) -> std::result::Result<Option<(usize, Self::DeMessage)>, Self::DeError> {
        let de = serde_json::Deserializer::from_slice(src);
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
    smol::future::block_on(async {
        let client = ClientBuilder::new_with_codec("tcp://127.0.0.1:6000", CustomJsonCodec {})
            .expect("Create client builder")
            .build()
            .await
            .expect("Create rpc client");

        loop {
            Timer::after(Duration::from_millis(100)).await;
            let result: Pong = client
                .call("Calc.ping", ())
                .await
                .expect("Call Calc.ping method");
            info!("Ping result:  {:?}", result);
        }
    });
}
