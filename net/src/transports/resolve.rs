use std::{net::SocketAddr, time::Duration};

use karyon_core::{async_runtime::net::lookup_host, async_util::timeout};

use crate::{Addr, Endpoint, Error, Result};

/// Max time to wait for a DNS lookup.
const DNS_TIMEOUT: Duration = Duration::from_secs(10);

/// Async version of `SocketAddr::try_from(endpoint)`. Domains are resolved
/// on a blocking thread pool under a timeout, so DNS never blocks the
/// executor.
pub(crate) async fn resolve_endpoint(endpoint: &Endpoint) -> Result<SocketAddr> {
    let port = endpoint.port()?;
    match endpoint.addr()? {
        Addr::Ip(ip) => Ok(SocketAddr::new(ip, port)),
        Addr::Domain(d) => timeout(DNS_TIMEOUT, lookup_host(&d, port))
            .await??
            .into_iter()
            .next()
            .ok_or_else(|| Error::ParseEndpoint(format!("could not resolve {d}:{port}"))),
    }
}

#[cfg(test)]
mod tests {
    use karyon_core::testing::run_test;

    use super::*;

    #[test]
    fn test_resolve_endpoint() {
        run_test(10, async {
            let ep: Endpoint = "tcp://127.0.0.1:3000".parse().unwrap();
            let addr = resolve_endpoint(&ep).await.unwrap();
            assert_eq!(addr, "127.0.0.1:3000".parse().unwrap());

            // A domain goes through the async lookup.
            let ep: Endpoint = "tcp://localhost:3000".parse().unwrap();
            let addr = resolve_endpoint(&ep).await.unwrap();
            assert!(addr.ip().is_loopback());
            assert_eq!(addr.port(), 3000);

            let ep: Endpoint = "unix:///tmp/karyon.sock".parse().unwrap();
            assert!(resolve_endpoint(&ep).await.is_err());
        });
    }
}
