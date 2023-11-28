use std::{
    net::{IpAddr, SocketAddr},
    os::unix::net::SocketAddr as UnixSocketAddress,
    path::PathBuf,
    str::FromStr,
};

use bincode::{Decode, Encode};
use url::Url;

use crate::{Error, Result};

/// Port defined as a u16.
pub type Port = u16;

/// Endpoint defines generic network endpoints for karyons.
///
/// # Example
///
/// ```
/// use std::net::SocketAddr;
///
/// use karyons_net::Endpoint;
///
/// let endpoint: Endpoint = "tcp://127.0.0.1:3000".parse().unwrap();
///
/// let socketaddr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
/// let endpoint =  Endpoint::new_udp_addr(&socketaddr);
///
/// ```
///
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Endpoint {
    Udp(Addr, Port),
    Tcp(Addr, Port),
    Tls(Addr, Port),
    Unix(String),
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Endpoint::Udp(ip, port) => {
                write!(f, "udp://{}:{}", ip, port)
            }
            Endpoint::Tcp(ip, port) => {
                write!(f, "tcp://{}:{}", ip, port)
            }
            Endpoint::Tls(ip, port) => {
                write!(f, "tls://{}:{}", ip, port)
            }
            Endpoint::Unix(path) => {
                if path.is_empty() {
                    write!(f, "unix:/UNNAMED")
                } else {
                    write!(f, "unix:/{}", path)
                }
            }
        }
    }
}

impl TryFrom<Endpoint> for SocketAddr {
    type Error = Error;
    fn try_from(endpoint: Endpoint) -> std::result::Result<SocketAddr, Self::Error> {
        match endpoint {
            Endpoint::Udp(ip, port) | Endpoint::Tcp(ip, port) | Endpoint::Tls(ip, port) => {
                Ok(SocketAddr::new(ip.try_into()?, port))
            }
            Endpoint::Unix(_) => Err(Error::TryFromEndpoint),
        }
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

impl TryFrom<Endpoint> for UnixSocketAddress {
    type Error = Error;
    fn try_from(endpoint: Endpoint) -> std::result::Result<UnixSocketAddress, Self::Error> {
        match endpoint {
            Endpoint::Unix(a) => Ok(UnixSocketAddress::from_pathname(a)?),
            _ => Err(Error::TryFromEndpoint),
        }
    }
}

impl FromStr for Endpoint {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let url: Url = match s.parse() {
            Ok(u) => u,
            Err(err) => return Err(Error::ParseEndpoint(err.to_string())),
        };

        if url.has_host() {
            let host = url.host_str().unwrap();

            let addr = match host.parse::<IpAddr>() {
                Ok(addr) => Addr::Ip(addr),
                Err(_) => Addr::Domain(host.to_string()),
            };

            let port = match url.port() {
                Some(p) => p,
                None => return Err(Error::ParseEndpoint(format!("port missing: {s}"))),
            };

            match url.scheme() {
                "tcp" => Ok(Endpoint::Tcp(addr, port)),
                "udp" => Ok(Endpoint::Udp(addr, port)),
                "tls" => Ok(Endpoint::Tls(addr, port)),
                _ => Err(Error::InvalidEndpoint(s.to_string())),
            }
        } else {
            if url.path().is_empty() {
                return Err(Error::InvalidEndpoint(s.to_string()));
            }

            match url.scheme() {
                "unix" => Ok(Endpoint::Unix(url.path().to_string())),
                _ => Err(Error::InvalidEndpoint(s.to_string())),
            }
        }
    }
}

impl Endpoint {
    /// Creates a new TCP endpoint from a `SocketAddr`.
    pub fn new_tcp_addr(addr: &SocketAddr) -> Endpoint {
        Endpoint::Tcp(Addr::Ip(addr.ip()), addr.port())
    }

    /// Creates a new TLS endpoint from a `SocketAddr`.
    pub fn new_tls_addr(addr: &SocketAddr) -> Endpoint {
        Endpoint::Tls(Addr::Ip(addr.ip()), addr.port())
    }

    /// Creates a new UDP endpoint from a `SocketAddr`.
    pub fn new_udp_addr(addr: &SocketAddr) -> Endpoint {
        Endpoint::Udp(Addr::Ip(addr.ip()), addr.port())
    }

    /// Creates a new Unix endpoint from a `UnixSocketAddress`.
    pub fn new_unix_addr(addr: &UnixSocketAddress) -> Endpoint {
        Endpoint::Unix(
            addr.as_pathname()
                .and_then(|a| a.to_str())
                .unwrap_or("")
                .to_string(),
        )
    }

    /// Returns the `Port` of the endpoint.
    pub fn port(&self) -> Result<&Port> {
        match self {
            Endpoint::Udp(_, port) | Endpoint::Tcp(_, port) | Endpoint::Tls(_, port) => Ok(port),
            _ => Err(Error::TryFromEndpoint),
        }
    }

    /// Returns the `Addr` of the endpoint.
    pub fn addr(&self) -> Result<&Addr> {
        match self {
            Endpoint::Udp(addr, _) | Endpoint::Tcp(addr, _) | Endpoint::Tls(addr, _) => Ok(addr),
            _ => Err(Error::TryFromEndpoint),
        }
    }
}

/// Addr defines a type for an address, either IP or domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Encode, Decode)]
pub enum Addr {
    Ip(IpAddr),
    Domain(String),
}

impl TryFrom<Addr> for IpAddr {
    type Error = Error;
    fn try_from(addr: Addr) -> std::result::Result<IpAddr, Self::Error> {
        match addr {
            Addr::Ip(ip) => Ok(ip),
            Addr::Domain(d) => Err(Error::InvalidAddress(d)),
        }
    }
}

impl std::fmt::Display for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Addr::Ip(ip) => {
                write!(f, "{}", ip)
            }
            Addr::Domain(d) => {
                write!(f, "{}", d)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

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
        let endpoint = Endpoint::Unix("/home/x/s.socket".to_string());
        assert_eq!(endpoint_str, endpoint);
    }
}
