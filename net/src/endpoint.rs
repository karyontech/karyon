use std::{
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    path::PathBuf,
    str::FromStr,
};

#[cfg(all(target_family = "unix", feature = "unix"))]
use std::os::unix::net::SocketAddr as UnixSocketAddr;

use bincode::{Decode, Encode};
use url::Url;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Port defined as a u16.
pub type Port = u16;

/// Endpoint defines generic network endpoints for karyon.
///
/// Transport schemes (TCP, UDP, TLS, QUIC, Unix) carry just address + port.
/// URL schemes (HTTP, HTTPS, WS, WSS) carry a full `Url` so path, query,
/// and other URL semantics are preserved.
///
/// # Example
///
/// ```
/// use std::net::SocketAddr;
///
/// use karyon_net::Endpoint;
///
/// let endpoint: Endpoint = "tcp://127.0.0.1:3000".parse().unwrap();
///
/// let socketaddr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
/// let endpoint = Endpoint::new_udp_addr(socketaddr);
///
/// let endpoint: Endpoint = "http://example.com:8080/rpc".parse().unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(into = "String"))]
pub enum Endpoint {
    Udp(Addr, Port),
    Tcp(Addr, Port),
    Tls(Addr, Port),
    Quic(Addr, Port),
    Http(Url),
    Https(Url),
    Ws(Url),
    Wss(Url),
    Unix(PathBuf),
}

impl Endpoint {
    /// Creates a new TCP endpoint from a `SocketAddr`.
    pub fn new_tcp_addr(addr: SocketAddr) -> Endpoint {
        Endpoint::Tcp(Addr::Ip(addr.ip()), addr.port())
    }

    /// Creates a new UDP endpoint from a `SocketAddr`.
    pub fn new_udp_addr(addr: SocketAddr) -> Endpoint {
        Endpoint::Udp(Addr::Ip(addr.ip()), addr.port())
    }

    /// Creates a new TLS endpoint from a `SocketAddr`.
    pub fn new_tls_addr(addr: SocketAddr) -> Endpoint {
        Endpoint::Tls(Addr::Ip(addr.ip()), addr.port())
    }

    /// Creates a new QUIC endpoint from a `SocketAddr`.
    pub fn new_quic_addr(addr: SocketAddr) -> Endpoint {
        Endpoint::Quic(Addr::Ip(addr.ip()), addr.port())
    }

    /// Creates a new Unix endpoint from a `UnixSocketAddr`.
    pub fn new_unix_addr(addr: &std::path::Path) -> Endpoint {
        Endpoint::Unix(addr.to_path_buf())
    }

    #[inline]
    /// Checks if the `Endpoint` is of type `Tcp`.
    pub fn is_tcp(&self) -> bool {
        matches!(self, Endpoint::Tcp(..))
    }

    #[inline]
    /// Checks if the `Endpoint` is of type `Tls`.
    pub fn is_tls(&self) -> bool {
        matches!(self, Endpoint::Tls(..))
    }

    #[inline]
    /// Checks if the `Endpoint` is of type `Ws`.
    pub fn is_ws(&self) -> bool {
        matches!(self, Endpoint::Ws(..))
    }

    #[inline]
    /// Checks if the `Endpoint` is of type `Wss`.
    pub fn is_wss(&self) -> bool {
        matches!(self, Endpoint::Wss(..))
    }

    #[inline]
    /// Checks if the `Endpoint` is of type `Quic`.
    pub fn is_quic(&self) -> bool {
        matches!(self, Endpoint::Quic(..))
    }

    #[inline]
    /// Checks if the `Endpoint` is of type `Udp`.
    pub fn is_udp(&self) -> bool {
        matches!(self, Endpoint::Udp(..))
    }

    #[inline]
    /// Checks if the `Endpoint` is of type `Http`.
    pub fn is_http(&self) -> bool {
        matches!(self, Endpoint::Http(..))
    }

    #[inline]
    /// Checks if the `Endpoint` is of type `Https`.
    pub fn is_https(&self) -> bool {
        matches!(self, Endpoint::Https(..))
    }

    #[inline]
    /// Checks if the `Endpoint` is of type `Unix`.
    pub fn is_unix(&self) -> bool {
        matches!(self, Endpoint::Unix(..))
    }

    /// Returns the port of the endpoint. For URL-family endpoints the
    /// scheme's default port is returned if the URL omits a port.
    pub fn port(&self) -> Result<Port> {
        match self {
            Endpoint::Tcp(_, port)
            | Endpoint::Udp(_, port)
            | Endpoint::Tls(_, port)
            | Endpoint::Quic(_, port) => Ok(*port),
            Endpoint::Http(url) | Endpoint::Https(url) | Endpoint::Ws(url) | Endpoint::Wss(url) => {
                url.port_or_known_default()
                    .ok_or_else(|| Error::ParseEndpoint(format!("port missing: {url}")))
            }
            Endpoint::Unix(_) => Err(Error::TryFromEndpoint),
        }
    }

    /// Returns the address of the endpoint.
    pub fn addr(&self) -> Result<Addr> {
        match self {
            Endpoint::Tcp(addr, _)
            | Endpoint::Udp(addr, _)
            | Endpoint::Tls(addr, _)
            | Endpoint::Quic(addr, _) => Ok(addr.clone()),
            Endpoint::Http(url) | Endpoint::Https(url) | Endpoint::Ws(url) | Endpoint::Wss(url) => {
                url_to_addr(url)
            }
            Endpoint::Unix(_) => Err(Error::TryFromEndpoint),
        }
    }
}

fn url_to_addr(url: &Url) -> Result<Addr> {
    let host = url
        .host_str()
        .ok_or_else(|| Error::ParseEndpoint(format!("host missing: {url}")))?;
    match host.parse::<IpAddr>() {
        Ok(ip) => Ok(Addr::Ip(ip)),
        Err(_) => Ok(Addr::Domain(host.to_string())),
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Endpoint::Udp(ip, port) => write!(f, "udp://{ip}:{port}"),
            Endpoint::Tcp(ip, port) => write!(f, "tcp://{ip}:{port}"),
            Endpoint::Tls(ip, port) => write!(f, "tls://{ip}:{port}"),
            Endpoint::Quic(ip, port) => write!(f, "quic://{ip}:{port}"),
            Endpoint::Http(url) | Endpoint::Https(url) | Endpoint::Ws(url) | Endpoint::Wss(url) => {
                write!(f, "{url}")
            }
            Endpoint::Unix(path) => write!(f, "unix:/{}", path.to_string_lossy()),
        }
    }
}

impl From<Endpoint> for String {
    fn from(endpoint: Endpoint) -> String {
        endpoint.to_string()
    }
}

impl TryFrom<Endpoint> for SocketAddr {
    type Error = Error;
    fn try_from(endpoint: Endpoint) -> std::result::Result<SocketAddr, Self::Error> {
        match endpoint {
            Endpoint::Udp(addr, port)
            | Endpoint::Tcp(addr, port)
            | Endpoint::Tls(addr, port)
            | Endpoint::Quic(addr, port) => resolve(addr, port),
            Endpoint::Http(ref url)
            | Endpoint::Https(ref url)
            | Endpoint::Ws(ref url)
            | Endpoint::Wss(ref url) => {
                let addr = url_to_addr(url)?;
                let port = endpoint.port()?;
                resolve(addr, port)
            }
            Endpoint::Unix(_) => Err(Error::TryFromEndpoint),
        }
    }
}

/// Resolve an `Addr` + port to a `SocketAddr`. Domains are resolved
/// through the system DNS with the given port.
fn resolve(addr: Addr, port: Port) -> Result<SocketAddr> {
    match addr {
        Addr::Ip(ip) => Ok(SocketAddr::new(ip, port)),
        Addr::Domain(d) => (d.as_str(), port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| Error::ParseEndpoint(format!("could not resolve {d}:{port}"))),
    }
}

impl TryFrom<Endpoint> for PathBuf {
    type Error = Error;
    fn try_from(endpoint: Endpoint) -> std::result::Result<PathBuf, Self::Error> {
        match endpoint {
            Endpoint::Unix(path) => Ok(PathBuf::from(&path)),
            _ => Err(Error::TryFromEndpoint),
        }
    }
}

#[cfg(all(feature = "unix", target_family = "unix"))]
impl TryFrom<Endpoint> for UnixSocketAddr {
    type Error = Error;
    fn try_from(endpoint: Endpoint) -> std::result::Result<UnixSocketAddr, Self::Error> {
        match endpoint {
            Endpoint::Unix(a) => Ok(UnixSocketAddr::from_pathname(a)?),
            _ => Err(Error::TryFromEndpoint),
        }
    }
}

impl TryFrom<Endpoint> for Url {
    type Error = Error;

    fn try_from(ep: Endpoint) -> Result<Self> {
        match ep {
            Endpoint::Http(u) | Endpoint::Https(u) | Endpoint::Ws(u) | Endpoint::Wss(u) => Ok(u),
            other => other
                .to_string()
                .parse::<Url>()
                .map_err(|e| Error::ParseEndpoint(e.to_string())),
        }
    }
}

impl TryFrom<Url> for Endpoint {
    type Error = Error;

    fn try_from(url: Url) -> Result<Self> {
        match url.scheme() {
            "http" => Ok(Endpoint::Http(url)),
            "https" => Ok(Endpoint::Https(url)),
            "ws" => Ok(Endpoint::Ws(url)),
            "wss" => Ok(Endpoint::Wss(url)),
            "tcp" | "udp" | "tls" | "quic" => {
                let addr = url_to_addr(&url)?;
                let port = url
                    .port()
                    .ok_or_else(|| Error::ParseEndpoint(format!("port missing: {url}")))?;
                Ok(match url.scheme() {
                    "tcp" => Endpoint::Tcp(addr, port),
                    "udp" => Endpoint::Udp(addr, port),
                    "tls" => Endpoint::Tls(addr, port),
                    "quic" => Endpoint::Quic(addr, port),
                    _ => unreachable!(),
                })
            }
            "unix" => {
                if url.path().is_empty() {
                    return Err(Error::UnsupportedEndpoint(url.to_string()));
                }
                Ok(Endpoint::Unix(url.path().into()))
            }
            _ => Err(Error::UnsupportedEndpoint(url.to_string())),
        }
    }
}

impl FromStr for Endpoint {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let url: Url = s
            .parse()
            .map_err(|err: url::ParseError| Error::ParseEndpoint(err.to_string()))?;
        Endpoint::try_from(url)
    }
}

/// Addr defines a type for an address, either IP or domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Addr {
    Ip(IpAddr),
    Domain(String),
}

impl TryFrom<Addr> for IpAddr {
    type Error = std::io::Error;
    fn try_from(addr: Addr) -> std::result::Result<IpAddr, Self::Error> {
        match addr {
            Addr::Ip(ip) => Ok(ip),
            Addr::Domain(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Addr::Domain cannot be converted to IpAddr without a port; \
                 use SocketAddr::try_from(Endpoint) instead",
            )),
        }
    }
}

impl std::fmt::Display for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Addr::Ip(ip) => write!(f, "{ip}"),
            Addr::Domain(d) => write!(f, "{d}"),
        }
    }
}

pub trait ToEndpoint {
    fn to_endpoint(&self) -> Result<Endpoint>;
}

impl ToEndpoint for String {
    fn to_endpoint(&self) -> Result<Endpoint> {
        Endpoint::from_str(self)
    }
}

impl ToEndpoint for Endpoint {
    fn to_endpoint(&self) -> Result<Endpoint> {
        Ok(self.clone())
    }
}

impl ToEndpoint for &str {
    fn to_endpoint(&self) -> Result<Endpoint> {
        Endpoint::from_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::path::PathBuf;

    #[test]
    fn test_endpoint_from_str() {
        let endpoint_str: Endpoint = "tcp://127.0.0.1:3000".parse().unwrap();
        let endpoint = Endpoint::Tcp(Addr::Ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))), 3000);
        assert_eq!(endpoint_str, endpoint);

        let endpoint_str: Endpoint = "udp://127.0.0.1:4000".parse().unwrap();
        let endpoint = Endpoint::Udp(Addr::Ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))), 4000);
        assert_eq!(endpoint_str, endpoint);

        let endpoint_str: Endpoint = "tcp://example.com:3000".parse().unwrap();
        let endpoint = Endpoint::Tcp(Addr::Domain("example.com".to_string()), 3000);
        assert_eq!(endpoint_str, endpoint);

        let endpoint_str = "unix:/home/x/s.socket".parse::<Endpoint>().unwrap();
        let endpoint = Endpoint::Unix(PathBuf::from_str("/home/x/s.socket").unwrap());
        assert_eq!(endpoint_str, endpoint);
    }

    #[test]
    fn test_endpoint_url_preserves_path() {
        let endpoint: Endpoint = "http://example.com:8080/rpc?x=1".parse().unwrap();
        match &endpoint {
            Endpoint::Http(url) => {
                assert_eq!(url.path(), "/rpc");
                assert_eq!(url.query(), Some("x=1"));
                assert_eq!(url.port(), Some(8080));
            }
            _ => panic!("expected Http"),
        }
        assert_eq!(endpoint.port().unwrap(), 8080);
    }

    #[test]
    fn test_endpoint_url_default_port() {
        let endpoint: Endpoint = "https://example.com/".parse().unwrap();
        assert_eq!(endpoint.port().unwrap(), 443);

        let endpoint: Endpoint = "ws://example.com/".parse().unwrap();
        assert_eq!(endpoint.port().unwrap(), 80);
    }
}
