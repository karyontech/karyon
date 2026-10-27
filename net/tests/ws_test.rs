#![cfg(feature = "ws")]

use std::mem::replace;

use karyon_core::{async_runtime::spawn, testing::run_test};
use karyon_net::{
    codec::Codec, layers::ws::Message, tcp, ClientLayer, Endpoint, Error, ServerLayer, WsLayer,
};

/// Sends and receives raw bytes as binary WS messages.
#[derive(Clone)]
struct RawCodec;

impl Codec<Message> for RawCodec {
    type Message = Vec<u8>;
    type Error = Error;

    fn encode(&self, src: &Vec<u8>, dst: &mut Message) -> Result<usize, Error> {
        *dst = Message::Binary(src.clone());
        Ok(src.len())
    }

    fn decode(&self, src: &mut Message) -> Result<Option<(usize, Vec<u8>)>, Error> {
        let bytes = replace(src, Message::Close).into_bytes();
        Ok(Some((bytes.len(), bytes)))
    }
}

#[test]
fn ws_rejects_message_over_limit() {
    run_test(10, async {
        let ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();
        let listener = tcp::TcpListener::bind(&ep, Default::default())
            .await
            .unwrap();
        let server_ep = listener.local_endpoint().unwrap();

        let server = spawn(async move {
            let stream = listener.accept().await.unwrap();
            let layer = WsLayer::server(RawCodec).with_max_msg_size(1024);
            let mut conn = ServerLayer::handshake(&layer, stream).await.unwrap();

            // Under the limit: received.
            assert_eq!(conn.recv_msg().await.unwrap(), vec![1u8; 512]);
            // Over the limit: rejected.
            conn.recv_msg().await.is_err()
        });

        let stream = tcp::connect(&server_ep, Default::default()).await.unwrap();
        let url = format!("ws://127.0.0.1:{}", server_ep.port().unwrap());
        let layer = WsLayer::client(&url, RawCodec);
        let mut conn = ClientLayer::handshake(&layer, stream).await.unwrap();

        conn.send_msg(vec![1u8; 512]).await.unwrap();
        let _ = conn.send_msg(vec![2u8; 2048]).await;

        assert!(server.await.unwrap());
    });
}
