use std::{sync::Arc, time::Duration};

use async_channel::{Receiver, Sender};
use async_trait::async_trait;
use bincode::{Decode, Encode};
use log::trace;
use rand::{rngs::OsRng, RngCore};

use karyon_core::{
    async_runtime::Executor,
    async_util::{select, sleep, timeout, Either, TaskGroup, TaskResult},
    util::decode,
};

use crate::{
    peer::Peer,
    protocol::{Protocol, ProtocolEvent, ProtocolID},
    version::Version,
    Error, Result,
};

const MAX_FAILUERS: u32 = 3;

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
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        peer: Arc<Peer>,
        ping_interval: u64,
        ping_timeout: u64,
        executor: Executor,
    ) -> Arc<dyn Protocol> {
        Arc::new(Self {
            peer,
            ping_interval,
            ping_timeout,
            task_group: TaskGroup::with_executor(executor),
        })
    }

    async fn recv_loop(&self, pong_chan: Sender<[u8; 32]>) -> Result<()> {
        loop {
            let event = self.peer.recv::<Self>().await?;
            let msg_payload = match event.clone() {
                ProtocolEvent::Message(m) => m,
                ProtocolEvent::Shutdown => {
                    break;
                }
            };

            let (msg, _) = decode::<PingProtocolMsg>(&msg_payload)?;

            match msg {
                PingProtocolMsg::Ping(nonce) => {
                    trace!("Received Ping message {:?}", nonce);
                    self.peer
                        .send(Self::id(), &PingProtocolMsg::Pong(nonce))
                        .await?;
                    trace!("Send back Pong message {:?}", nonce);
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

        while retry < MAX_FAILUERS {
            sleep(Duration::from_secs(self.ping_interval)).await;

            let mut ping_nonce: [u8; 32] = [0; 32];
            rng.fill_bytes(&mut ping_nonce);

            trace!("Send Ping message {:?}", ping_nonce);
            self.peer
                .send(Self::id(), &PingProtocolMsg::Ping(ping_nonce))
                .await?;

            // Wait for Pong message
            let d = Duration::from_secs(self.ping_timeout);
            let pong_msg = match timeout(d, chan.recv()).await {
                Ok(m) => m?,
                Err(_) => {
                    retry += 1;
                    continue;
                }
            };
            trace!("Received Pong message {:?}", pong_msg);

            if pong_msg != ping_nonce {
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
        let (pong_chan, pong_chan_recv) = async_channel::bounded(1);

        self.task_group.spawn(
            {
                let this = self.clone();
                async move { this.ping_loop(pong_chan_recv.clone()).await }
            },
            |res| async move {
                if let TaskResult::Completed(result) = res {
                    let _ = stop_signal.0.send(result).await;
                }
            },
        );

        let result = select(self.recv_loop(pong_chan), stop_signal.1.recv()).await;
        self.task_group.cancel().await;

        match result {
            Either::Left(res) => {
                trace!("Receive loop stopped {:?}", res);
                res
            }
            Either::Right(res) => {
                let res = res?;
                trace!("Ping loop stopped {:?}", res);
                res
            }
        }
    }

    fn version() -> Result<Version> {
        "0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "PING".into()
    }
}
