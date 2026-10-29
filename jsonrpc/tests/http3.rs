#![cfg(feature = "http3")]

#[path = "shared/positional.rs"]
mod positional;
mod shared;

use std::{future::poll_fn, net::SocketAddr, sync::Arc, time::Duration};

use bytes::{Buf, Bytes};
use h3::client::{new as h3_client, RequestStream, SendRequest};
use h3_quinn::{BidiStream, Connection, OpenStreams};
use hyper::Request;
use serde_json::{json, Deserializer, Value};

use karyon_core::{async_runtime::spawn, async_util::timeout, testing::run_test};

use karyon_jsonrpc::{
    client::ClientBuilder,
    server::{Server, ServerBuilder},
};
use karyon_net::{
    quic::{ClientQuicConfig, QuicConn, QuicEndpoint, ServerQuicConfig},
    Endpoint,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use shared::{generate_test_certs, MathService};

/// Must match `MathService::blob_subscribe`.
const BLOB_COUNT: usize = 5;
const BLOB_SIZE: usize = 64 * 1024;

const RECV_TIMEOUT: Duration = Duration::from_secs(2);

fn test_quic_configs() -> (ServerQuicConfig, ClientQuicConfig) {
    let (cert_chain, private_key): (Vec<CertificateDer>, std::sync::Arc<PrivateKeyDer>) =
        generate_test_certs();
    let server = ServerQuicConfig::new(cert_chain.clone(), private_key);
    let client = ClientQuicConfig::new(cert_chain, "localhost");
    (server, client)
}

type H3Stream = RequestStream<BidiStream<Bytes>, Bytes>;

/// Start an HTTP/3 server with `MathService` as RPC and pubsub service.
async fn start_server() -> (Arc<Server>, Endpoint, ClientQuicConfig) {
    let service = Arc::new(MathService {});
    let (srv_cfg, cli_cfg) = test_quic_configs();

    let server = ServerBuilder::new("http://127.0.0.1:0")
        .expect("Create server builder")
        .service(service.clone())
        .pubsub_service(service)
        .quic_config(srv_cfg)
        .expect("Set quic config")
        .build()
        .await
        .expect("Build server");

    let endpoint = server.local_endpoint().expect("Get local endpoint");
    server.clone().start();
    (server, endpoint, cli_cfg)
}

/// Dial the server with a raw h3 client. The h3 driver runs in the
/// background.
async fn h3_connect(
    endpoint: &Endpoint,
    config: ClientQuicConfig,
) -> (QuicConn, SendRequest<OpenStreams, Bytes>) {
    let addr = SocketAddr::try_from(endpoint.clone()).expect("Socket address");
    let quic_conn = QuicEndpoint::dial(&Endpoint::new_quic_addr(addr), config)
        .await
        .expect("Dial QUIC");
    let (mut driver, send_request) = h3_client(Connection::new(quic_conn.inner().clone()))
        .await
        .expect("H3 handshake");
    spawn(async move {
        let _ = poll_fn(|cx| driver.poll_close(cx)).await;
    })
    .detach();
    (quic_conn, send_request)
}

/// Open a POST request stream and send `msg` without finishing the body.
async fn h3_post(send_request: &mut SendRequest<OpenStreams, Bytes>, msg: Value) -> H3Stream {
    let req = Request::post("https://localhost/")
        .body(())
        .expect("Build request");
    let mut stream = send_request.send_request(req).await.expect("Send request");
    stream
        .send_data(Bytes::from(msg.to_string()))
        .await
        .expect("Send body");
    stream
}

/// Next JSON message on the response stream.
async fn next_msg(stream: &mut H3Stream, buf: &mut Vec<u8>) -> Value {
    loop {
        let mut iter = Deserializer::from_slice(buf).into_iter::<Value>();
        if let Some(Ok(msg)) = iter.next() {
            let n = iter.byte_offset();
            buf.drain(..n);
            return msg;
        }
        let mut chunk = stream
            .recv_data()
            .await
            .expect("Recv data")
            .expect("Stream ended");
        while chunk.has_remaining() {
            let n = chunk.chunk().len();
            buf.extend_from_slice(chunk.chunk());
            chunk.advance(n);
        }
    }
}

#[test]
fn rpc_call() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .quic_config(cli_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build client");

        let result: String = client
            .call("MathService.ping", ())
            .await
            .expect("Call ping");
        assert_eq!(result, "pong");

        let result: i32 = client
            .call("MathService.add", serde_json::json!({"x": 10, "y": 20}))
            .await
            .expect("Call add");
        assert_eq!(result, 30);

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn rpc_error_handling() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .quic_config(cli_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build client");

        let result: Result<Value, _> = client.call("MathService.error_method", ()).await;
        assert!(result.is_err());

        let result: Result<Value, _> = client.call("MathService.nonexistent", ()).await;
        assert!(result.is_err());

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn multiple_clients() {
    run_test(15, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let mut clients = Vec::new();
        for _ in 0..3 {
            let client = ClientBuilder::new(endpoint.to_string())
                .expect("Create client builder")
                .quic_config(cli_cfg.clone())
                .expect("Set quic config")
                .build()
                .await
                .expect("Build client");
            clients.push(client);
        }

        for (i, client) in clients.iter().enumerate() {
            let x = i as i32;
            let result: i32 = client
                .call("MathService.add", serde_json::json!({"x": x, "y": x}))
                .await
                .expect("Call add");
            assert_eq!(result, x * 2);
        }

        for client in &clients {
            client.stop().await;
        }
        server.shutdown().await;
    });
}

#[test]
fn pubsub_subscription() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service.clone())
            .pubsub_service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .quic_config(cli_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build client");

        let sub = client
            .subscribe("MathService.counter_subscribe", ())
            .await
            .expect("Subscribe");

        let msg1: i32 = serde_json::from_value(sub.recv().await.expect("Recv notification 1"))
            .expect("Parse notification");
        let msg2: i32 = serde_json::from_value(sub.recv().await.expect("Recv notification 2"))
            .expect("Parse notification");

        assert_eq!(msg1, 1);
        assert_eq!(msg2, 2);

        client
            .unsubscribe("MathService.counter_unsubscribe", sub.id())
            .await
            .expect("Unsubscribe");

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn pubsub_large_notifications() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service.clone())
            .pubsub_service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .quic_config(cli_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build client");

        let sub = client
            .subscribe("MathService.blob_subscribe", ())
            .await
            .expect("Subscribe");

        for i in 0..BLOB_COUNT {
            let nt = timeout(RECV_TIMEOUT, sub.recv())
                .await
                .expect("Notification timed out")
                .expect("Recv notification");
            assert_eq!(nt["i"], i);
            assert_eq!(nt["data"].as_str().map(|d| d.len()), Some(BLOB_SIZE));
        }

        client.stop().await;
        server.shutdown().await;
    });
}

/// Unsubscribe is sent on the subscription's own stream, so the server
/// finds the subscription.
#[test]
fn unsubscribe_finds_subscription() {
    run_test(10, async {
        let (server, endpoint, cli_cfg) = start_server().await;
        let (_conn, mut send_request) = h3_connect(&endpoint, cli_cfg).await;

        let sub = json!({
            "jsonrpc": "2.0", "id": 1, "method": "MathService.counter_subscribe", "params": null
        });
        let mut stream = h3_post(&mut send_request, sub).await;
        stream.recv_response().await.expect("Recv response");

        let mut buf = Vec::new();
        let res = next_msg(&mut stream, &mut buf).await;
        let sub_id = res["result"].clone();

        let unsub = json!({
            "jsonrpc": "2.0", "id": 2, "method": "MathService.counter_unsubscribe", "params": sub_id
        });
        stream
            .send_data(Bytes::from(unsub.to_string()))
            .await
            .expect("Send unsubscribe");
        stream.finish().await.expect("Finish request");

        // Skip notifications until the unsubscribe response.
        loop {
            let msg = next_msg(&mut stream, &mut buf).await;
            if msg["id"] == json!(2) {
                assert_eq!(msg["result"], json!(true));
                break;
            }
        }

        server.shutdown().await;
    });
}

/// A call whose params look like a subscription id does not close that
/// subscription.
#[test]
fn u32_params_keep_subscription() {
    run_test(10, async {
        let (server, endpoint, cli_cfg) = start_server().await;

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .quic_config(cli_cfg)
            .expect("Set quic config")
            .build()
            .await
            .expect("Build client");

        let sub = client
            .subscribe("MathService.counter_subscribe", ())
            .await
            .expect("Subscribe");
        sub.recv().await.expect("Recv notification");

        let echoed: u32 = client
            .call("MathService.echo", sub.id())
            .await
            .expect("Call echo");
        assert_eq!(echoed, sub.id());

        // The subscription keeps streaming.
        for _ in 0..5 {
            timeout(RECV_TIMEOUT, sub.recv())
                .await
                .expect("Notification timed out")
                .expect("Recv notification");
        }

        client.stop().await;
        server.shutdown().await;
    });
}

/// A stream that never sends its headers does not block other requests
/// on the same connection.
#[test]
fn stalled_stream_does_not_block_connection() {
    run_test(10, async {
        let (server, endpoint, cli_cfg) = start_server().await;
        let (conn, mut send_request) = h3_connect(&endpoint, cli_cfg).await;

        // Only the HEADERS frame type byte, never the rest of the frame.
        let (mut stalled, _recv) = conn.inner().open_bi().await.expect("Open stream");
        stalled.write_all(&[0x01]).await.expect("Write frame type");

        let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "MathService.ping", "params": null});
        let res = timeout(RECV_TIMEOUT, async {
            let mut stream = h3_post(&mut send_request, ping).await;
            stream.finish().await.expect("Finish request");
            stream.recv_response().await.expect("Recv response");
            next_msg(&mut stream, &mut Vec::new()).await
        })
        .await
        .expect("Request timed out");
        assert_eq!(res["result"], json!("pong"));

        server.shutdown().await;
    });
}

/// A server with a custom notification format and a client with the
/// matching decoder.
#[test]
fn custom_notification_format() {
    run_test(10, async {
        let service = Arc::new(MathService {});
        let (srv_cfg, cli_cfg) = test_quic_configs();

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .pubsub_service(service)
            .quic_config(srv_cfg)
            .expect("Set quic config")
            .with_notification_encoder(positional::encode)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .quic_config(cli_cfg)
            .expect("Set quic config")
            .with_notification_decoder(positional::decode)
            .build()
            .await
            .expect("Build client");

        let sub = client
            .subscribe("MathService.counter_subscribe", ())
            .await
            .expect("Subscribe");

        let msg1: i32 = serde_json::from_value(sub.recv().await.expect("Recv notification 1"))
            .expect("Parse notification");
        let msg2: i32 = serde_json::from_value(sub.recv().await.expect("Recv notification 2"))
            .expect("Parse notification");
        assert_eq!(msg1, 1);
        assert_eq!(msg2, 2);

        client.stop().await;
        server.shutdown().await;
    });
}
