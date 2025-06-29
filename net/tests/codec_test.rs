use karyon_net::{
    codec::{BytesCodec, Codec, Decoder, Encoder},
    tcp::{dial, listen, TcpConfig},
    ConnListener, Connection, Endpoint,
};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const DEFAULT_MAX_SIZE: usize = 8 * 1024 * 1024; // 8MB

#[derive(Clone)]
pub struct LinesCodec {
    max_size: usize,
}

impl Default for LinesCodec {
    fn default() -> Self {
        Self {
            max_size: DEFAULT_MAX_SIZE,
        }
    }
}

impl Codec for LinesCodec {
    type Error = karyon_net::Error;
    type Message = String;
}

impl Encoder for LinesCodec {
    type EnError = karyon_net::Error;
    type EnMessage = String;
    fn encode(
        &self,
        src: &Self::EnMessage,
        dst: &mut karyon_net::codec::ByteBuffer,
    ) -> std::result::Result<usize, Self::EnError> {
        if dst.len() >= self.max_size {
            return Err(karyon_net::Error::IO(std::io::ErrorKind::Other.into()));
        }

        let mut src = src.as_bytes().to_vec();
        src.push(b'\n');
        dst.extend_from_slice(&src);
        Ok(src.len())
    }
}

impl Decoder for LinesCodec {
    type DeError = karyon_net::Error;
    type DeMessage = String;
    fn decode(
        &self,
        src: &mut karyon_net::codec::ByteBuffer,
    ) -> std::result::Result<Option<(usize, Self::DeMessage)>, Self::DeError> {
        if src.len() >= self.max_size {
            return Err(karyon_net::Error::IO(std::io::ErrorKind::Other.into()));
        }

        if src.is_empty() {
            Ok(None)
        } else {
            let msg_bytes: Vec<u8> = src.as_ref().to_vec();
            let newline_offset = match msg_bytes.iter().position(|b| *b == b'\n') {
                Some(o) => o,
                None => return Ok(None),
            };
            let msg = String::from_utf8_lossy(&msg_bytes[..newline_offset]).to_string();
            Ok(Some((newline_offset + 1, msg)))
        }
    }
}

#[test]
fn test_codec_tcp_basic() {
    smol::block_on(async move {
        let endpoint = Endpoint::from_str("tcp://127.0.0.1:6001").unwrap();
        let config = TcpConfig::default();
        let codec = LinesCodec::default();
        let listener = listen(&endpoint, config.clone(), codec.clone())
            .await
            .unwrap();

        let task = smol::spawn(async move {
            loop {
                let c = listener.accept().await.unwrap();
                let _ = c.recv().await.unwrap();
            }
        });

        // Give server time to start
        smol::Timer::after(std::time::Duration::from_millis(500)).await;

        let conn = dial(&endpoint, config, codec).await.unwrap();
        conn.send("hello".to_string()).await.unwrap();
        let _ = task.cancel().await;
    });
}

#[test]
fn test_codec_tcp_aggressive_messaging() {
    smol::block_on(async move {
        let endpoint = Endpoint::from_str("tcp://127.0.0.1:6002").unwrap();
        let config = TcpConfig::default();
        let codec = LinesCodec::default();
        let listener = listen(&endpoint, config.clone(), codec.clone())
            .await
            .unwrap();

        let received_count = Arc::new(AtomicUsize::new(0));
        let received_count_clone = received_count.clone();

        let server_task = smol::spawn(async move {
            loop {
                let conn = listener.accept().await.unwrap();
                let count = received_count_clone.clone();

                smol::spawn(async move {
                    loop {
                        match conn.recv().await {
                            Ok(_) => {
                                count.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(_) => break,
                        }
                    }
                })
                .detach();
            }
        });

        // Give server time to start
        smol::Timer::after(std::time::Duration::from_millis(500)).await;

        // Create multiple concurrent connections
        let mut client_tasks = Vec::new();
        let num_connections = 5;
        let messages_per_connection = 100;

        for conn_id in 0..num_connections {
            let endpoint = endpoint.clone();
            let config = config.clone();
            let codec = codec.clone();

            let client_task = smol::spawn(async move {
                let conn = dial(&endpoint, config, codec).await.unwrap();

                // Send messages rapidly
                for msg_id in 0..messages_per_connection {
                    let message = format!("conn-{}-msg-{}", conn_id, msg_id);
                    conn.send(message).await.unwrap();

                    // Minimal delay to create aggressive timing
                    if msg_id % 10 == 0 {
                        smol::Timer::after(std::time::Duration::from_millis(1)).await;
                    }
                }
            });

            client_tasks.push(client_task);
        }

        // Wait for all clients to finish sending
        for task in client_tasks {
            task.await;
        }

        // Give some time for all messages to be processed
        smol::Timer::after(std::time::Duration::from_secs(2)).await;

        let total_expected = num_connections * (messages_per_connection);
        let total_received = received_count.load(Ordering::Relaxed);

        assert_eq!(
            total_received, total_expected,
            "Not all messages were received"
        );

        server_task.cancel().await;
    });
}

#[test]
fn test_codec_tcp_max_payload_size() {
    smol::block_on(async move {
        let endpoint = Endpoint::from_str("tcp://127.0.0.1:6003").unwrap();
        let config = TcpConfig::default();
        let codec = BytesCodec::new(DEFAULT_MAX_SIZE);

        let listener = listen(&endpoint, config.clone(), codec.clone())
            .await
            .unwrap();

        let server_task = smol::spawn(async move {
            let conn = listener.accept().await.unwrap();

            loop {
                match conn.recv().await {
                    Ok(msg) => {
                        conn.send(msg).await.unwrap();
                    }
                    Err(_) => break,
                }
            }
        });

        // Give server time to start
        smol::Timer::after(std::time::Duration::from_millis(500)).await;

        let conn = dial(&endpoint, config, codec).await.unwrap();

        // Test different payload sizes
        let test_sizes = vec![
            (1, true),
            (1024, true),              // 1KB
            (64 * 1024, true),         // 64KB
            (1024 * 1024, true),       // 1MB
            (4 * 1024 * 1024, true),   // 4MB
            (16 * 1024 * 1024, false), // 16MB
        ];

        for (size, pass) in &test_sizes {
            println!("Testing payload size: {} bytes", size);

            let msg = vec![1u8; *size];

            // Send the payload
            match conn.send(msg.clone()).await {
                Ok(_) => {
                    assert!(pass);
                }
                Err(_) => {
                    assert!(!pass);
                    continue;
                }
            }

            // Wait for confirmation
            let response = conn.recv().await.unwrap();

            assert_eq!(
                response.len(),
                msg.len(),
                "Server didn't confirm reception of {} message",
                size
            );

            // Small delay between large messages
            smol::Timer::after(std::time::Duration::from_millis(500)).await;
        }

        server_task.cancel().await.unwrap();
    });
}
