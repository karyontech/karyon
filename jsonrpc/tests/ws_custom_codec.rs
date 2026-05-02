//! Integration test for a custom WebSocket codec.
//! The codec encodes JSON values as WebSocket Binary frames (instead
//! of the default Text frames) to verify the JsonRpcWsCodec path on
//! both server and client.

#![cfg(feature = "ws")]

mod shared;

use std::{io, sync::Arc};

use karyon_core::testing::run_test;

use karyon_jsonrpc::{client::ClientBuilder, codec::Codec, server::ServerBuilder};
use karyon_net::layers::ws::Message as WsMessage;
use karyon_net::Error as NetError;

use shared::MathService;

/// JSON over WebSocket Binary frames.
#[derive(Clone)]
struct BinaryWsCodec;

impl Codec<WsMessage> for BinaryWsCodec {
    type Message = serde_json::Value;
    type Error = NetError;

    fn encode(&self, src: &serde_json::Value, dst: &mut WsMessage) -> Result<usize, NetError> {
        let bytes = serde_json::to_vec(src).map_err(|e| NetError::IO(io::Error::other(e)))?;
        let n = bytes.len();
        *dst = WsMessage::Binary(bytes);
        Ok(n)
    }

    fn decode(&self, src: &mut WsMessage) -> Result<Option<(usize, serde_json::Value)>, NetError> {
        match src {
            WsMessage::Binary(b) => {
                let n = b.len();
                let val: serde_json::Value =
                    serde_json::from_slice(b).map_err(|e| NetError::IO(io::Error::other(e)))?;
                Ok(Some((n, val)))
            }
            // Text frames produced by the default JsonCodec are not
            // expected on this connection. Ignore other control frames.
            _ => Ok(None),
        }
    }
}

#[test]
fn custom_ws_codec_rpc_call() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new("ws://127.0.0.1:0")
            .expect("server builder")
            .with_ws_codec(BinaryWsCodec)
            .service(service)
            .build()
            .await
            .expect("build server");

        let ep = server.local_endpoint().expect("endpoint");
        server.clone().start();

        let client = ClientBuilder::new(ep.to_string())
            .expect("client builder")
            .with_ws_codec(BinaryWsCodec)
            .build()
            .await
            .expect("build client");

        let r: String = client.call("MathService.ping", ()).await.expect("ping");
        assert_eq!(r, "pong");

        let r: i32 = client
            .call("MathService.add", serde_json::json!({"x": 11, "y": 31}))
            .await
            .expect("add");
        assert_eq!(r, 42);

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn custom_ws_codec_pubsub() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new("ws://127.0.0.1:0")
            .expect("server builder")
            .with_ws_codec(BinaryWsCodec)
            .service(service.clone())
            .pubsub_service(service)
            .build()
            .await
            .expect("build server");

        let ep = server.local_endpoint().expect("endpoint");
        server.clone().start();

        let client = ClientBuilder::new(ep.to_string())
            .expect("client builder")
            .with_ws_codec(BinaryWsCodec)
            .build()
            .await
            .expect("build client");

        let sub = client
            .subscribe("MathService.counter_subscribe", ())
            .await
            .expect("subscribe");

        let v: i32 = serde_json::from_value(sub.recv().await.expect("nt")).expect("parse");
        assert!(v >= 1);

        client
            .unsubscribe("MathService.counter_unsubscribe", sub.id())
            .await
            .expect("unsubscribe");

        client.stop().await;
        server.shutdown().await;
    });
}
