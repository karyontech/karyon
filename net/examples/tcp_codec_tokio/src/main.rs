use std::time::Duration;

use karyon_core::async_util::sleep;

use karyon_net::{
    codec::{Codec, Decoder, Encoder},
    tcp, ConnListener, Connection, Endpoint, Error, Result,
};

#[derive(Clone)]
struct NewLineCodec {}

impl Codec for NewLineCodec {
    type Message = String;
    type Error = Error;
}

impl Encoder for NewLineCodec {
    type EnMessage = String;
    type EncodeError = Error;
    fn encode(&self, src: &Self::EnMessage, dst: &mut [u8]) -> Result<usize> {
        dst[..src.len()].copy_from_slice(src.as_bytes());
        Ok(src.len())
    }
}

impl Decoder for NewLineCodec {
    type DeMessage = String;
    type DecodeError = Error;
    fn decode(&self, src: &mut [u8]) -> Result<Option<(usize, Self::DeMessage)>> {
        match src.iter().position(|&b| b == b'\n') {
            Some(i) => Ok(Some((i + 1, String::from_utf8(src[..i].to_vec()).unwrap()))),
            None => Ok(None),
        }
    }
}

#[tokio::main]
async fn main() {
    let endpoint: Endpoint = "tcp://127.0.0.1:3000".parse().unwrap();

    let config = tcp::TcpConfig::default();

    let listener = tcp::listen(&endpoint, config.clone(), NewLineCodec {})
        .await
        .unwrap();
    tokio::spawn(async move {
        if let Ok(conn) = listener.accept().await {
            loop {
                let msg = conn.recv().await.unwrap();
                println!("Receive a message: {:?}", msg);
            }
        };
    });

    let conn = tcp::dial(&endpoint, config, NewLineCodec {}).await.unwrap();
    conn.send("hello".to_string()).await.unwrap();
    conn.send(" world\n".to_string()).await.unwrap();
    sleep(Duration::from_secs(1)).await;
}
