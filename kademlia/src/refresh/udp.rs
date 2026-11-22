//! The UDP refresh: Ping/Pong datagrams to each entry's UDP address, and
//! a listener that answers them. Not on wasm.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use log::{info, trace};
use rand::{TryRng, rngs::SysRng};

use karyon_core::async_util::Backoff;

use karyon_net::{Endpoint, udp};

use karyon_p2p::{
    Action, ConnDirection, Error, Result, Subject,
    monitor::ConnectionKind,
    util::{decode, encode},
};

use crate::messages::RefreshMsg;

use super::RefreshService;

// Max UDP datagram payload size for refresh messages.
const MAX_UDP_BUF: usize = 1024;

// Token-bucket parameters for the per-IP rate limit on incoming pings.
// Capacity allows small bursts; refill rate caps sustained traffic.
const RL_CAPACITY: u32 = 5;
const RL_REFILL_PER_SEC: f64 = 0.5;

/// Per-IP token bucket used by `listen_to_ping_msg` to drop floods.
struct RateBucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateBucket {
    fn new() -> Self {
        Self {
            tokens: RL_CAPACITY as f64,
            last_refill: Instant::now(),
        }
    }

    /// Refill and try to take one token. Returns true if allowed.
    fn allow(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * RL_REFILL_PER_SEC).min(RL_CAPACITY as f64);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl RefreshService {
    /// Pings the peer at `endpoint` over UDP, with retries.
    pub(super) async fn udp_ping(&self, endpoint: Endpoint) -> Result<()> {
        let mut retry = 0;
        let conn = udp::dial(&endpoint, Default::default()).await?;
        let peer_addr = SocketAddr::try_from(endpoint.clone())?;
        let backoff = Backoff::with_executor(100, 5000, self.task_group.executor().clone());
        while retry < self.kconfig.refresh_connect_retries {
            match self.send_ping_msg(&conn, peer_addr).await {
                Ok(()) => return Ok(()),
                Err(Error::Timeout) => {
                    retry += 1;
                    backoff.sleep().await;
                }
                Err(err) => {
                    return Err(err);
                }
            }
        }
        Err(Error::Timeout)
    }

    /// Listen on UDP for Ping messages.
    pub(super) async fn listen_loop(self: Arc<Self>, endpoint: Endpoint) -> Result<()> {
        let conn = match udp::listen(&endpoint, Default::default()).await {
            Ok(c) => {
                self.monitor
                    .notify(ConnectionKind::Listening(endpoint.clone()))
                    .await;
                c
            }
            Err(err) => {
                self.monitor
                    .notify(ConnectionKind::ListenFailed(endpoint.clone()))
                    .await;
                return Err(err.into());
            }
        };
        info!("Start listening on {endpoint}");

        // Per-IP rate limit. Lives on the listen task, no lock needed:
        // listen_to_ping_msg is only ever called from this loop.
        let mut rate_limiter: HashMap<IpAddr, RateBucket> = HashMap::new();

        loop {
            let res = self.listen_to_ping_msg(&conn, &mut rate_limiter).await;
            if let Err(err) = res {
                trace!("Failed to handle ping msg {err}");
                self.monitor.notify(ConnectionKind::AcceptFailed).await;
            }
        }
    }

    /// Listen for a Ping message and respond with a Pong message.
    /// Drops pings from sources not in the routing table or that
    /// exceed the per-IP rate limit.
    async fn listen_to_ping_msg(
        &self,
        conn: &udp::UdpConn,
        rate_limiter: &mut HashMap<IpAddr, RateBucket>,
    ) -> Result<()> {
        let mut buf = vec![0u8; MAX_UDP_BUF];
        let (n, sender) = conn.recv_from(&mut buf).await?;

        let sender_ip = sender.ip();
        if !self.table.has_discovery_ip(&sender_ip) {
            trace!("Drop refresh ping from unknown source {sender_ip}");
            return Ok(());
        }

        let allowed = rate_limiter
            .entry(sender_ip)
            .or_insert_with(RateBucket::new)
            .allow();
        if !allowed {
            trace!("Drop rate-limited refresh ping from {sender_ip}");
            return Ok(());
        }

        let sender_ep = Endpoint::new_udp_addr(sender);

        if !self.config.access_control.allow(
            &Subject::Endpoint(&sender_ep),
            Action::Probe(ConnDirection::Inbound),
        ) {
            trace!("Drop refresh ping from denied source {sender_ip}");
            return Ok(());
        }

        self.monitor
            .notify(ConnectionKind::Accepted(sender_ep.clone()))
            .await;

        let (msg, _) = decode::<RefreshMsg>(&buf[..n])?;
        match msg {
            RefreshMsg::Ping(m) => {
                let pong_msg = RefreshMsg::Pong(m);
                let encoded = encode(&pong_msg)?;
                conn.send_to(&encoded, sender).await?;
            }
            RefreshMsg::Pong(_) => return Err(Error::InvalidMsg("Unexpected pong msg".into())),
        }

        self.monitor
            .notify(ConnectionKind::Disconnected(sender_ep))
            .await;
        Ok(())
    }

    /// Sends a Ping msg and waits for the Pong response.
    async fn send_ping_msg(&self, conn: &udp::UdpConn, peer_addr: SocketAddr) -> Result<()> {
        let mut nonce: [u8; 32] = [0; 32];
        SysRng.try_fill_bytes(&mut nonce)?;

        let ping = RefreshMsg::Ping(nonce);
        let encoded = encode(&ping)?;
        conn.send_to(&encoded, peer_addr).await?;

        let t = Duration::from_secs(self.kconfig.refresh_response_timeout);
        let mut buf = vec![0u8; MAX_UDP_BUF];
        let (n, _) = self
            .task_group
            .executor()
            .timeout(t, conn.recv_from(&mut buf))
            .await??;
        let (msg, _) = decode::<RefreshMsg>(&buf[..n])?;

        match msg {
            RefreshMsg::Pong(n) => {
                if n != nonce {
                    return Err(Error::InvalidPongMsg);
                }
                Ok(())
            }
            _ => Err(Error::InvalidMsg("Unexpected ping msg".into())),
        }
    }
}
