mod shared;

use std::sync::Arc;

use serde_json::Value;

use karyon_core::{
    async_runtime::{
        io::{AsyncReadExt, AsyncWriteExt},
        spawn,
    },
    testing::run_test,
};

use karyon_jsonrpc::{
    client::{ClientBuilder, HttpConfig},
    server::{HttpConfig as ServerHttpConfig, ServerBuilder},
};
use karyon_net::{
    tcp::{self, TcpListener},
    Endpoint,
};

use shared::MathService;

#[test]
fn rpc_call() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
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

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .build()
            .await
            .expect("Build client");

        let result: Result<Value, _> = client.call("MathService.error_method", ()).await;
        assert!(result.is_err());

        let result: Result<Value, _> = client.call("MathService.nonexistent", ()).await;
        assert!(result.is_err());

        let result: Result<Value, _> = client.call("Unknown.method", ()).await;
        assert!(result.is_err());

        client.stop().await;
        server.shutdown().await;
    });
}

#[test]
fn multiple_clients() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let mut clients = vec![];
        for _ in 0..5 {
            let client = ClientBuilder::new(endpoint.to_string())
                .expect("Create client builder")
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
            assert_eq!(result, x + x);
        }

        for client in &clients {
            client.stop().await;
        }
        server.shutdown().await;
    });
}

/// The connection preface every HTTP/2 client sends first.
const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// A plain TCP listener that stands in for an HTTP server, so tests can
/// read the raw bytes a client sends.
async fn raw_listener() -> (TcpListener, u16) {
    let ep: Endpoint = "tcp://127.0.0.1:0".parse().expect("Parse endpoint");
    let listener = TcpListener::bind(&ep, Default::default())
        .await
        .expect("Bind listener");
    let port = listener
        .local_endpoint()
        .expect("Local endpoint")
        .port()
        .expect("Port");
    (listener, port)
}

fn http2_config() -> HttpConfig {
    HttpConfig {
        http2: true,
        ..Default::default()
    }
}

#[test]
fn http2_rpc_call() {
    run_test(10, async {
        let service = Arc::new(MathService {});

        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(service)
            .build()
            .await
            .expect("Build server");

        let endpoint = server.local_endpoint().expect("Get local endpoint");
        server.clone().start();

        let client = ClientBuilder::new(endpoint.to_string())
            .expect("Create client builder")
            .http_config(http2_config())
            .expect("Enable HTTP/2")
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

/// The client really speaks HTTP/2: the first bytes on the wire
/// are the HTTP/2 preface.
#[test]
fn http2_sends_preface() {
    run_test(10, async {
        let (listener, port) = raw_listener().await;

        // build() opens the connection and starts the handshake.
        let _client = spawn(async move {
            let _ = ClientBuilder::new(format!("http://127.0.0.1:{port}"))
                .expect("Create client builder")
                .http_config(http2_config())
                .expect("Enable HTTP/2")
                .build()
                .await;
        });

        let mut stream = listener.accept().await.expect("Accept");
        let mut buf = [0u8; 24];
        stream.read_exact(&mut buf).await.expect("Read preface");
        assert_eq!(&buf[..], H2_PREFACE);
    });
}

#[test]
fn http2_needs_http_endpoint() {
    let builder = ClientBuilder::new("tcp://127.0.0.1:6000").expect("Create client builder");
    assert!(builder.http_config(http2_config()).is_err());
}

/// Accept one connection and return its request head, lowercased.
async fn read_request_head(listener: &TcpListener) -> String {
    let mut stream = listener.accept().await.expect("Accept");
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.expect("Read request");
        head.push(byte[0]);
    }
    String::from_utf8(head).expect("UTF-8 head").to_lowercase()
}

/// The HTTP/1.1 client sends the configured `Host` header.
#[test]
fn http1_sends_configured_host_header() {
    run_test(10, async {
        let (listener, port) = raw_listener().await;

        let _call = spawn(async move {
            let config = HttpConfig {
                host: Some("example.com".into()),
                ..Default::default()
            };
            let client = ClientBuilder::new(format!("http://127.0.0.1:{port}"))
                .expect("Create client builder")
                .http_config(config)
                .expect("Set http config")
                .build()
                .await
                .expect("Build client");
            let _: Result<Value, _> = client.call("MathService.ping", ()).await;
        });

        let head = read_request_head(&listener).await;
        assert!(head.contains("host: example.com\r\n"), "{head}");
    });
}

/// The client sends the configured `Content-Type`.
#[test]
fn client_sends_configured_content_type() {
    run_test(10, async {
        let (listener, port) = raw_listener().await;

        let _call = spawn(async move {
            let config = HttpConfig {
                content_type: "application/x-test".into(),
                ..Default::default()
            };
            let client = ClientBuilder::new(format!("http://127.0.0.1:{port}"))
                .expect("Create client builder")
                .http_config(config)
                .expect("Set http config")
                .build()
                .await
                .expect("Build client");
            let _: Result<Value, _> = client.call("MathService.ping", ()).await;
        });

        let head = read_request_head(&listener).await;
        assert!(
            head.contains("content-type: application/x-test\r\n"),
            "{head}"
        );
    });
}

/// The server answers with the configured `Content-Type`.
#[test]
fn server_sends_configured_content_type() {
    run_test(10, async {
        let config = ServerHttpConfig {
            content_type: "application/x-test".into(),
        };
        let server = ServerBuilder::new("http://127.0.0.1:0")
            .expect("Create server builder")
            .service(Arc::new(MathService {}))
            .http_config(config)
            .expect("Set http config")
            .build()
            .await
            .expect("Build server");
        let port = server
            .local_endpoint()
            .expect("Endpoint")
            .port()
            .expect("Port");
        server.clone().start();

        let ep: Endpoint = format!("tcp://127.0.0.1:{port}").parse().expect("Endpoint");
        let mut stream = tcp::connect(&ep, Default::default())
            .await
            .expect("Connect");
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"MathService.ping","params":null}"#;
        let req = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(req.as_bytes())
            .await
            .expect("Write request");

        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.expect("Read response");
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).expect("UTF-8 head").to_lowercase();
        assert!(
            head.contains("content-type: application/x-test\r\n"),
            "{head}"
        );

        server.shutdown().await;
    });
}
