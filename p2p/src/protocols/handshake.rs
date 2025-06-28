use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use log::trace;

use karyon_core::{async_util::timeout, util::decode};

use crate::{
    message::{NetMsg, NetMsgCmd, VerAckMsg, VerMsg},
    peer::Peer,
    protocol::{InitProtocol, ProtocolID},
    version::{version_match, VersionInt},
    Error, PeerID, Result, Version,
};

pub struct HandshakeProtocol {
    peer: Arc<Peer>,
    protocols: HashMap<ProtocolID, Version>,
}

#[async_trait]
impl InitProtocol for HandshakeProtocol {
    type T = Result<PeerID>;
    /// Initiate a handshake with a connection.
    async fn init(self: Arc<Self>) -> Self::T {
        trace!("Init Handshake: {}", self.peer.remote_endpoint());

        if !self.peer.is_inbound() {
            self.send_vermsg().await?;
        }

        let t = Duration::from_secs(self.peer.config().handshake_timeout);
        let msg: NetMsg = timeout(t, self.peer.conn.recv_inner()).await??;
        match msg.header.command {
            NetMsgCmd::Version => {
                let result = self.validate_version_msg(&msg).await;
                match result {
                    Ok(_) => {
                        self.send_verack(true).await?;
                    }
                    Err(Error::IncompatibleVersion(_)) | Err(Error::UnsupportedProtocol(_)) => {
                        self.send_verack(false).await?;
                    }
                    _ => {}
                };
                result
            }
            NetMsgCmd::Verack => self.validate_verack_msg(&msg).await,
            cmd => Err(Error::InvalidMsg(format!("unexpected msg found {cmd:?}"))),
        }
    }
}

impl HandshakeProtocol {
    pub fn new(peer: Arc<Peer>, protocols: HashMap<ProtocolID, Version>) -> Arc<Self> {
        Arc::new(Self { peer, protocols })
    }

    /// Sends a Version message
    async fn send_vermsg(&self) -> Result<()> {
        let protocols = self
            .protocols
            .clone()
            .into_iter()
            .map(|p| (p.0, p.1.v))
            .collect();

        let vermsg = VerMsg {
            peer_id: self.peer.own_id().clone(),
            protocols,
            version: self.peer.config().version.v.clone(),
        };

        trace!("Send VerMsg");
        self.peer
            .conn
            .send_inner(NetMsg::new(NetMsgCmd::Version, &vermsg)?)
            .await?;
        Ok(())
    }

    /// Sends a Verack message
    async fn send_verack(&self, ack: bool) -> Result<()> {
        let verack = VerAckMsg {
            peer_id: self.peer.own_id().clone(),
            ack,
        };

        trace!("Send VerAckMsg {verack:?}");
        self.peer
            .conn
            .send_inner(NetMsg::new(NetMsgCmd::Verack, &verack)?)
            .await?;
        Ok(())
    }

    /// Validates the given version msg
    async fn validate_version_msg(&self, msg: &NetMsg) -> Result<PeerID> {
        let (vermsg, _) = decode::<VerMsg>(&msg.payload)?;

        if !version_match(&self.peer.config().version.req, &vermsg.version) {
            return Err(Error::IncompatibleVersion("system: {}".into()));
        }

        self.protocols_match(&vermsg.protocols).await?;

        trace!("Received VerMsg from: {}", vermsg.peer_id);
        Ok(vermsg.peer_id)
    }

    /// Validates the given verack msg
    async fn validate_verack_msg(&self, msg: &NetMsg) -> Result<PeerID> {
        let (verack, _) = decode::<VerAckMsg>(&msg.payload)?;

        if !verack.ack {
            return Err(Error::IncompatiblePeer);
        }

        trace!("Received VerAckMsg from: {}", verack.peer_id);
        Ok(verack.peer_id)
    }

    /// Check if the new connection has compatible protocols.
    async fn protocols_match(&self, protocols: &HashMap<ProtocolID, VersionInt>) -> Result<()> {
        for (n, pv) in protocols.iter() {
            match self.protocols.get(n) {
                Some(v) => {
                    if !version_match(&v.req, pv) {
                        return Err(Error::IncompatibleVersion(format!("{n} protocol: {pv}")));
                    }
                }
                None => {
                    return Err(Error::UnsupportedProtocol(n.to_string()));
                }
            }
        }
        Ok(())
    }
}
