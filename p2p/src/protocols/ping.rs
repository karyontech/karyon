use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use async_trait::async_trait;
use bincode::{Decode, Encode};
use log::trace;
use rand::{TryRng, rngs::SysRng};

use karyon_core::{
    async_runtime::Executor,
    async_util::{Either, select},
};

use crate::{
    Error, Result,
    protocol::{PeerConn, Protocol, ProtocolFlags, ProtocolID},
    util::{decode, encode},
    version::Version,
};

const MAX_FAILURES: u32 = 3;

/// Recent ping nonces remembered to drop replays.
const PING_DEDUP_WINDOW: usize = 64;

/// Protocol id for ping. Mandatory in every handshake.
pub(crate) const PING_PROTO_ID: &str = "PING";

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
enum PingProtocolMsg {
    Ping([u8; 32]),
    Pong([u8; 32]),
}

/// Decode a `PingProtocolMsg` ensuring the entire payload was consumed.
fn decode_ping_msg(payload: &[u8]) -> Result<PingProtocolMsg> {
    let (msg, consumed) = decode::<PingProtocolMsg>(payload)?;
    if consumed != payload.len() {
        return Err(Error::InvalidMsg(format!(
            "PingProtocolMsg has trailing bytes: consumed {consumed} of {} bytes",
            payload.len()
        )));
    }
    Ok(msg)
}

pub struct PingProtocol {
    peer: PeerConn,
    ping_interval: u64,
    ping_timeout: u64,
    /// Runs the ping timers.
    executor: Executor,
}

impl PingProtocol {
    /// `ping_interval` and `ping_timeout` are in seconds.
    pub(crate) fn new(
        peer: PeerConn,
        ping_interval: u64,
        ping_timeout: u64,
        executor: Executor,
    ) -> Self {
        Self {
            ping_interval,
            ping_timeout,
            peer,
            executor,
        }
    }

    async fn recv_loop(&self, pong_chan: Sender<[u8; 32]>) -> Result<()> {
        // Ring buffer of recently-seen ping nonces. Drops replays so
        // an attacker can't elicit unlimited Pongs by resending one.
        let mut seen: VecDeque<[u8; 32]> = VecDeque::with_capacity(PING_DEDUP_WINDOW);

        loop {
            let payload = match self.peer.recv().await {
                Ok(bytes) => bytes,
                Err(Error::PeerShutdown) => break,
                Err(e) => return Err(e),
            };

            let msg = decode_ping_msg(&payload)?;
            match msg {
                PingProtocolMsg::Ping(nonce) => {
                    if seen.iter().any(|n| n == &nonce) {
                        trace!("Drop replayed Ping {nonce:?}");
                        continue;
                    }
                    if seen.len() == PING_DEDUP_WINDOW {
                        seen.pop_front();
                    }
                    seen.push_back(nonce);

                    trace!("Received Ping {nonce:?}");
                    let bytes = encode(&PingProtocolMsg::Pong(nonce))?;
                    self.peer.send(bytes).await?;
                    trace!("Sent Pong {nonce:?}");
                }
                PingProtocolMsg::Pong(nonce) => {
                    pong_chan.send(nonce).await?;
                }
            }
        }
        Ok(())
    }

    async fn ping_loop(&self, chan: Receiver<[u8; 32]>) -> Result<()> {
        let mut retry = 0;

        while retry < MAX_FAILURES {
            self.executor
                .sleep(Duration::from_secs(self.ping_interval))
                .await;

            let mut nonce: [u8; 32] = [0; 32];
            SysRng.try_fill_bytes(&mut nonce)?;

            trace!("Send Ping {nonce:?}");
            let bytes = encode(&PingProtocolMsg::Ping(nonce))?;
            self.peer.send(bytes).await?;

            let d = Duration::from_secs(self.ping_timeout);
            let pong = match self.executor.timeout(d, chan.recv()).await {
                Ok(m) => m?,
                Err(_) => {
                    retry += 1;
                    continue;
                }
            };
            trace!("Received Pong {pong:?}");

            if pong != nonce {
                retry += 1;
                continue;
            }
            retry = 0;
        }

        Err(Error::Timeout)
    }
}

#[async_trait]
impl Protocol for PingProtocol {
    async fn start(self: Arc<Self>) -> Result<()> {
        trace!("Start Ping protocol");

        let (pong_tx, pong_rx) = async_channel::bounded(1);

        // Both loops live inside this future, so they stop together when
        // it ends or is cancelled with the peer's tasks.
        match select(self.recv_loop(pong_tx), self.ping_loop(pong_rx)).await {
            Either::Left(res) => {
                trace!("Receive loop stopped {res:?}");
                res
            }
            Either::Right(res) => {
                trace!("Ping loop stopped {res:?}");
                res
            }
        }
    }

    fn version() -> Result<Version> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        PING_PROTO_ID.into()
    }

    fn flags() -> ProtocolFlags {
        ProtocolFlags::REQUIRED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_ping_msg_accepts_exact_payload() {
        let nonce = [7u8; 32];

        let ping_bytes = encode(&PingProtocolMsg::Ping(nonce)).unwrap();
        assert_eq!(
            decode_ping_msg(&ping_bytes).unwrap(),
            PingProtocolMsg::Ping(nonce)
        );

        let pong_bytes = encode(&PingProtocolMsg::Pong(nonce)).unwrap();
        assert_eq!(
            decode_ping_msg(&pong_bytes).unwrap(),
            PingProtocolMsg::Pong(nonce)
        );
    }

    #[test]
    fn decode_ping_msg_rejects_trailing_bytes() {
        let nonce = [9u8; 32];

        let mut ping_bytes = encode(&PingProtocolMsg::Ping(nonce)).unwrap();
        ping_bytes.push(0);
        assert!(matches!(
            decode_ping_msg(&ping_bytes),
            Err(Error::InvalidMsg(_))
        ));

        let mut pong_bytes = encode(&PingProtocolMsg::Pong(nonce)).unwrap();
        pong_bytes.push(0xff);
        assert!(matches!(
            decode_ping_msg(&pong_bytes),
            Err(Error::InvalidMsg(_))
        ));
    }
}
