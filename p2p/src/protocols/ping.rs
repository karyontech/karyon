use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use bincode::{Decode, Encode};
use log::trace;
use rand::{rngs::OsRng, RngCore};
use smol::{
    channel,
    channel::{Receiver, Sender},
    stream::StreamExt,
    Timer,
};

use karyons_core::{
    async_util::{select, timeout, Either, TaskGroup, TaskResult},
    event::EventListener,
    util::decode,
    GlobalExecutor,
};

use karyons_net::NetError;

use crate::{
    peer::ArcPeer,
    protocol::{ArcProtocol, Protocol, ProtocolEvent, ProtocolID},
    version::Version,
    Result,
};

const MAX_FAILUERS: u32 = 3;

#[derive(Clone, Debug, Encode, Decode)]
enum PingProtocolMsg {
    Ping([u8; 32]),
    Pong([u8; 32]),
}

pub struct PingProtocol {
    peer: ArcPeer,
    ping_interval: u64,
    ping_timeout: u64,
    task_group: TaskGroup<'static>,
}

impl PingProtocol {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(peer: ArcPeer, executor: GlobalExecutor) -> ArcProtocol {
        let ping_interval = peer.config().ping_interval;
        let ping_timeout = peer.config().ping_timeout;
        Arc::new(Self {
            peer,
            ping_interval,
            ping_timeout,
            task_group: TaskGroup::new(executor),
        })
    }

    async fn recv_loop(
        &self,
        listener: &EventListener<ProtocolID, ProtocolEvent>,
        pong_chan: Sender<[u8; 32]>,
    ) -> Result<()> {
        loop {
            let event = listener.recv().await?;
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
                        .send(&Self::id(), &PingProtocolMsg::Pong(nonce))
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

    async fn ping_loop(self: Arc<Self>, chan: Receiver<[u8; 32]>) -> Result<()> {
        let mut timer = Timer::interval(Duration::from_secs(self.ping_interval));
        let rng = &mut OsRng;
        let mut retry = 0;

        while retry < MAX_FAILUERS {
            timer.next().await;

            let mut ping_nonce: [u8; 32] = [0; 32];
            rng.fill_bytes(&mut ping_nonce);

            trace!("Send Ping message {:?}", ping_nonce);
            self.peer
                .send(&Self::id(), &PingProtocolMsg::Ping(ping_nonce))
                .await?;

            let d = Duration::from_secs(self.ping_timeout);

            // Wait for Pong message
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
        }

        Err(NetError::Timeout.into())
    }
}

#[async_trait]
impl Protocol for PingProtocol {
    async fn start(self: Arc<Self>) -> Result<()> {
        trace!("Start Ping protocol");

        let (pong_chan, pong_chan_recv) = channel::bounded(1);
        let (stop_signal_s, stop_signal) = channel::bounded::<Result<()>>(1);

        let selfc = self.clone();
        self.task_group.spawn(
            selfc.clone().ping_loop(pong_chan_recv.clone()),
            |res| async move {
                if let TaskResult::Completed(result) = res {
                    let _ = stop_signal_s.send(result).await;
                }
            },
        );

        let listener = self.peer.register_listener::<Self>().await;

        let result = select(self.recv_loop(&listener, pong_chan), stop_signal.recv()).await;
        listener.cancel().await;
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
