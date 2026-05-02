use std::net::{Ipv4Addr, Ipv6Addr};

use karyon_core::async_runtime::io::{AsyncReadExt, AsyncWriteExt};

use crate::{layer::ClientLayer, ByteStream, Error, Result};

const SOCKS5_VERSION: u8 = 0x05;
const NO_AUTH: u8 = 0x00;
const CMD_CONNECT: u8 = 0x01;
const ATYPE_IPV4: u8 = 0x01;
const ATYPE_DOMAIN: u8 = 0x03;
const ATYPE_IPV6: u8 = 0x04;
const REPLY_SUCCESS: u8 = 0x00;

/// SOCKS5 proxy layer (RFC 1928). Client-only.
///
/// Performs the SOCKS5 handshake on the stream (which is already
/// connected to the proxy) to tunnel through to the target.
///
/// # Example
///
/// ```no_run
/// use karyon_net::{tcp, ClientLayer, Endpoint};
/// use karyon_net::layers::proxy::Socks5Layer;
///
/// async {
///     // Connect to the proxy
///     let proxy_ep: Endpoint = "tcp://127.0.0.1:1080".parse().unwrap();
///     let stream = tcp::connect(&proxy_ep, Default::default()).await.unwrap();
///
///     // Tunnel to target via SOCKS5
///     let layer = Socks5Layer::new("example.com", 443);
///     let tunneled = ClientLayer::handshake(&layer, stream).await.unwrap();
/// };
/// ```
#[derive(Clone)]
pub struct Socks5Layer {
    target_host: String,
    target_port: u16,
}

impl Socks5Layer {
    /// Create a new SOCKS5 layer targeting the given host and port.
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            target_host: host.to_string(),
            target_port: port,
        }
    }
}

impl ClientLayer<Box<dyn ByteStream>, Box<dyn ByteStream>> for Socks5Layer {
    async fn handshake(&self, mut stream: Box<dyn ByteStream>) -> Result<Box<dyn ByteStream>> {
        // Greeting: version + 1 method (no auth).
        stream.write_all(&[SOCKS5_VERSION, 1, NO_AUTH]).await?;
        stream.flush().await?;

        // Server picks a method.
        let mut resp = [0u8; 2];
        stream.read_exact(&mut resp).await?;
        if resp[0] != SOCKS5_VERSION || resp[1] != NO_AUTH {
            return Err(Error::Socks5("server rejected auth method".into()));
        }

        // Build connect request.
        let host = self.target_host.as_bytes();
        let port = self.target_port.to_be_bytes();

        let mut req = vec![SOCKS5_VERSION, CMD_CONNECT, 0x00];
        if let Ok(ip) = self.target_host.parse::<Ipv4Addr>() {
            req.push(ATYPE_IPV4);
            req.extend_from_slice(&ip.octets());
        } else if let Ok(ip) = self.target_host.parse::<Ipv6Addr>() {
            req.push(ATYPE_IPV6);
            req.extend_from_slice(&ip.octets());
        } else {
            req.push(ATYPE_DOMAIN);
            req.push(host.len() as u8);
            req.extend_from_slice(host);
        }
        req.extend_from_slice(&port);

        stream.write_all(&req).await?;
        stream.flush().await?;

        // Read reply header (4 bytes minimum).
        let mut reply = [0u8; 4];
        stream.read_exact(&mut reply).await?;
        if reply[0] != SOCKS5_VERSION {
            return Err(Error::Socks5("invalid reply version".into()));
        }
        if reply[1] != REPLY_SUCCESS {
            return Err(Error::Socks5(format!("connect failed (code {})", reply[1])));
        }

        // Skip the bound address in the reply.
        match reply[3] {
            ATYPE_IPV4 => {
                let mut skip = [0u8; 4 + 2]; // 4 ip + 2 port
                stream.read_exact(&mut skip).await?;
            }
            ATYPE_DOMAIN => {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await?;
                let mut skip = vec![0u8; len[0] as usize + 2];
                stream.read_exact(&mut skip).await?;
            }
            ATYPE_IPV6 => {
                let mut skip = [0u8; 16 + 2];
                stream.read_exact(&mut skip).await?;
            }
            atype => {
                return Err(Error::Socks5(format!(
                    "unknown address type in reply: {atype:#x}"
                )));
            }
        }

        Ok(stream)
    }
}
