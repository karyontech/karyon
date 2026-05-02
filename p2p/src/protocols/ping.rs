use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use async_trait::async_trait;
use bincode::{Decode, Encode};
use log::trace;
use rand::{rngs::OsRng, RngCore};

use karyon_core::{
    async_runtime::Executor,
    async_util::{select, sleep, timeout, Either, TaskGroup, TaskResult},
};

use crate::{
    peer::Peer,
    protocol::{Protocol, ProtocolEvent, ProtocolID, ProtocolKind},
    util::decode,
    version::Version,
    Error, Result,
};

const MAX_FAILURES: u32 = 3;

/// Recent ping nonces remembered to drop replays.
const PING_DEDUP_WINDOW: usize = 64;

/// Protocol id for ping. Mandatory in every handshake.
pub(crate) const PING_PROTO_ID: &str = "PING";

#[derive(Clone, Debug, Encode, Decode)]
enum PingProtocolMsg {
    Ping([u8; 32]),
    Pong([u8; 32]),
}

pub struct PingProtocol {
    peer: Arc<Peer>,
    ping_interval: u64,
    ping_timeout: u64,
    task_group: TaskGroup,
}

impl PingProtocol {
    pub fn new(
        peer: Arc<Peer>,
        ping_interval: u64,
        ping_timeout: u64,
        executor: Executor,
    ) -> Arc<Self> {
        Arc::new(Self {
            peer,
            ping_interval,
            ping_timeout,
            task_group: TaskGroup::with_executor(executor),
        })
    }

    async fn recv_loop(&self, pong_chan: Sender<[u8; 32]>) -> Result<()> {
        // Ring buffer of recently-seen ping nonces. Drops replays so
        // an attacker can't elicit unlimited Pongs by resending one.
        let mut seen: VecDeque<[u8; 32]> = VecDeque::with_capacity(PING_DEDUP_WINDOW);

        loop {
            let event = self.peer.recv::<Self>().await?;
            let payload = match event.clone() {
                ProtocolEvent::Message(m) => m,
                ProtocolEvent::Shutdown => break,
            };

            let (msg, _) = decode::<PingProtocolMsg>(&payload)?;
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
                    self.peer
                        .send(Self::id(), &PingProtocolMsg::Pong(nonce))
                        .await?;
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
        let rng = &mut OsRng;
        let mut retry = 0;

        while retry < MAX_FAILURES {
            sleep(Duration::from_secs(self.ping_interval)).await;

            let mut nonce: [u8; 32] = [0; 32];
            rng.fill_bytes(&mut nonce);

            trace!("Send Ping {nonce:?}");
            self.peer
                .send(Self::id(), &PingProtocolMsg::Ping(nonce))
                .await?;

            let d = Duration::from_secs(self.ping_timeout);
            let pong = match timeout(d, chan.recv()).await {
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

        let stop_signal = async_channel::bounded::<Result<()>>(1);
        let (pong_tx, pong_rx) = async_channel::bounded(1);

        self.task_group.spawn(
            {
                let this = self.clone();
                async move { this.ping_loop(pong_rx).await }
            },
            |res| async move {
                if let TaskResult::Completed(result) = res {
                    let _ = stop_signal.0.send(result).await;
                }
            },
        );

        let result = select(self.recv_loop(pong_tx), stop_signal.1.recv()).await;
        self.task_group.cancel().await;

        match result {
            Either::Left(res) => {
                trace!("Receive loop stopped {res:?}");
                res
            }
            Either::Right(res) => {
                let res = res?;
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

    fn kind() -> ProtocolKind {
        ProtocolKind::Mandatory
    }
}
