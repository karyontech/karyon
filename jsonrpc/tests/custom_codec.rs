//! Integration test for custom codec.
//! Uses a newline-delimited JSON codec (NDJSON) instead of the default
//! JsonCodec to verify the server/client generic C: JsonRpcCodec path.

mod shared;

use std::{io, sync::Arc};

use karyon_core::testing::run_test;

use karyon_jsonrpc::{
    client::ClientBuilder,
    codec::{ByteBuffer, Codec},
    server::ServerBuilder,
};
use karyon_net::Error as NetError;

use shared::MathService;

/// Newline-delimited JSON codec.
#[derive(Clone)]
struct NdJsonCodec;

impl Codec<ByteBuffer> for NdJsonCodec {
    type Message = serde_json::Value;
    type Error = NetError;

    fn encode(&self, src: &serde_json::Value, dst: &mut ByteBuffer) -> Result<usize, NetError> {
        let mut bytes = serde_json::to_vec(src).map_err(|e| NetError::IO(io::Error::other(e)))?;
        bytes.push(b'\n');
        let n = bytes.len();
        dst.extend_from_slice(&bytes);
        Ok(n)
    }

    fn decode(&self, src: &mut ByteBuffer) -> Result<Option<(usize, serde_json::Value)>, NetError> {
        let buf = src.as_ref();
        let Some(idx) = buf.iter().position(|b| *b == b'\n') else {
            return Ok(None);
        };
        let line = &buf[..idx];
        let val: serde_json::Value =
            serde_json::from_slice(line).map_err(|e| NetError::IO(io::Error::other(e)))?;
        Ok(Some((idx + 1, val)))
    }
}

#[test]
fn custom_codec_rpc_call() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new_with_codec("tcp://127.0.0.1:0", NdJsonCodec)
            .expect("server builder")
            .service(service)
            .build()
            .await
            .expect("build server");

        let ep = server.local_endpoint().expect("endpoint");
        server.clone().start();

        let client = ClientBuilder::new_with_codec(ep.to_string(), NdJsonCodec)
            .expect("client builder")
            .build()
            .await
            .expect("build client");

        let r: String = client.call("MathService.ping", ()).await.expect("ping");
        assert_eq!(r, "pong");

        let r: i32 = client
            .call("MathService.add", serde_json::json!({"x": 2, "y": 40}))
            .await
            .expect("add");
        assert_eq!(r, 42);

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn custom_codec_pubsub() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new_with_codec("tcp://127.0.0.1:0", NdJsonCodec)
            .expect("server builder")
            .service(service.clone())
            .pubsub_service(service)
            .build()
            .await
            .expect("build server");

        let ep = server.local_endpoint().expect("endpoint");
        server.clone().start();

        let client = ClientBuilder::new_with_codec(ep.to_string(), NdJsonCodec)
            .expect("client builder")
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
