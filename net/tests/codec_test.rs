use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use karyon_core::{async_util::sleep, testing::run_test};
use karyon_net::{
    codec::{Codec, LengthCodec},
    framed, tcp, ByteBuffer, Endpoint,
};

const DEFAULT_MAX_SIZE: usize = 8 * 1024 * 1024;

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

impl Codec<ByteBuffer> for LinesCodec {
    type Message = String;
    type Error = karyon_net::Error;

    fn encode(
        &self,
        src: &String,
        dst: &mut ByteBuffer,
    ) -> std::result::Result<usize, Self::Error> {
        if dst.len() >= self.max_size {
            return Err(karyon_net::Error::IO(std::io::ErrorKind::Other.into()));
        }
        let mut src = src.as_bytes().to_vec();
        src.push(b'\n');
        dst.extend_from_slice(&src);
        Ok(src.len())
    }

    fn decode(
        &self,
        src: &mut ByteBuffer,
    ) -> std::result::Result<Option<(usize, String)>, Self::Error> {
        if src.len() >= self.max_size {
            return Err(karyon_net::Error::IO(std::io::ErrorKind::Other.into()));
        }
        if src.is_empty() {
            Ok(None)
        } else {
            let bytes: Vec<u8> = src.as_ref().to_vec();
            let pos = match bytes.iter().position(|b| *b == b'\n') {
                Some(o) => o,
                None => return Ok(None),
            };
            let msg = String::from_utf8_lossy(&bytes[..pos]).to_string();
            Ok(Some((pos + 1, msg)))
        }
    }
}

#[test]
fn codec_tcp_basic() {
    run_test(10, async {
        let ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();

        let listener = tcp::TcpListener::bind(&ep, Default::default())
            .await
            .unwrap();
        let server_ep = listener.local_endpoint().unwrap();

        karyon_core::async_runtime::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let mut conn = framed(stream, LinesCodec::default());
            let _: String = conn.recv_msg().await.unwrap();
        })
        .detach();

        let stream = tcp::connect(&server_ep, Default::default()).await.unwrap();
        let mut conn = framed(stream, LinesCodec::default());
        conn.send_msg("hello".to_string()).await.unwrap();
    });
}

#[test]
fn codec_tcp_aggressive() {
    run_test(15, async {
        let ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();

        let listener = tcp::TcpListener::bind(&ep, Default::default())
            .await
            .unwrap();
        let server_ep = listener.local_endpoint().unwrap();

        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();

        karyon_core::async_runtime::spawn(async move {
            loop {
                let stream = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let c = count_clone.clone();
                karyon_core::async_runtime::spawn(async move {
                    let mut conn = framed(stream, LinesCodec::default());
                    loop {
                        match conn.recv_msg().await {
                            Ok::<String, _>(_) => {
                                c.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(_) => break,
                        }
                    }
                })
                .detach();
            }
        })
        .detach();

        let num_conns = 5usize;
        let msgs_per_conn = 100usize;

        let mut handles = Vec::new();
        for conn_id in 0..num_conns {
            let ep = server_ep.clone();
            let h = karyon_core::async_runtime::spawn(async move {
                let stream = tcp::connect(&ep, Default::default()).await.unwrap();
                let mut conn = framed(stream, LinesCodec::default());
                for msg_id in 0..msgs_per_conn {
                    let msg = format!("c{conn_id}-m{msg_id}");
                    conn.send_msg(msg).await.unwrap();
                    if msg_id % 10 == 0 {
                        sleep(std::time::Duration::from_millis(1)).await;
                    }
                }
            });
            handles.push(h);
        }

        for h in handles {
            let _ = h.await;
        }

        sleep(std::time::Duration::from_secs(2)).await;

        assert_eq!(count.load(Ordering::Relaxed), num_conns * msgs_per_conn,);
    });
}

#[test]
fn codec_tcp_max_payload() {
    run_test(15, async {
        let ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();

        let listener = tcp::TcpListener::bind(&ep, Default::default())
            .await
            .unwrap();
        let server_ep = listener.local_endpoint().unwrap();

        karyon_core::async_runtime::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let mut conn = framed(stream, LengthCodec::new(DEFAULT_MAX_SIZE));
            loop {
                match conn.recv_msg().await {
                    Ok::<Vec<u8>, _>(msg) => {
                        conn.send_msg(msg).await.unwrap();
                    }
                    Err(_) => break,
                }
            }
        })
        .detach();

        let stream = tcp::connect(&server_ep, Default::default()).await.unwrap();
        let mut conn = framed(stream, LengthCodec::new(DEFAULT_MAX_SIZE));

        let sizes = [1, 1024, 64 * 1024, 1024 * 1024, 4 * 1024 * 1024];
        for size in sizes {
            let msg = vec![1u8; size];
            conn.send_msg(msg.clone()).await.unwrap();
            let resp: Vec<u8> = conn.recv_msg().await.unwrap();
            assert_eq!(resp.len(), size);
        }
    });
}
