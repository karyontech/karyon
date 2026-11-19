use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::error;

use karyon_core::async_runtime::lock::RwLock;
use karyon_net::Endpoint;

use crate::{
    Error, PeerID, Result,
    conn_direction::ConnDirection,
    protocol::ProtocolID,
    session::{PeerSession, SessionInfo},
};

/// `Arc` reference to the `Sessions` table.
pub(crate) type SessionsRef = Arc<Sessions>;

/// The running peers, one session per peer id. Owned by the PeerPool,
/// shared with every protocol's `Broadcaster`.
pub(crate) struct Sessions {
    map: RwLock<HashMap<PeerID, PeerSession>>,
}

impl Sessions {
    pub(crate) fn new() -> SessionsRef {
        Arc::new(Self {
            map: RwLock::new(HashMap::new()),
        })
    }

    /// Adds the session. If a peer with this id is already in the table,
    /// the new connection either replaces it (its session is returned, for
    /// the caller to stop) or is rejected; see `new_conn_wins`.
    /// Connections are admitted in parallel, so the check and the insert
    /// run under one write lock.
    pub(crate) async fn insert(
        &self,
        own_id: &PeerID,
        session: PeerSession,
    ) -> Result<Option<PeerSession>> {
        let info = session.info.clone();
        let mut map = self.map.write().await;
        if let Some(old) = map.get(&info.id)
            && !new_conn_wins(own_id, &info.id, &old.info.direction, &info.direction)
        {
            return Err(Error::PeerAlreadyConnected);
        }
        Ok(map.insert(info.id.clone(), session))
    }

    /// Removes the session, unless the table holds another connection
    /// under its id. Returns whether it was removed.
    pub(crate) async fn remove(&self, session: &PeerSession) -> bool {
        let mut map = self.map.write().await;
        match map.get(&session.info.id) {
            Some(s) if s.conn_id == session.conn_id => map.remove(&session.info.id).is_some(),
            _ => false,
        }
    }

    /// Takes every session out of the table.
    pub(crate) async fn drain(&self) -> Vec<PeerSession> {
        self.map.write().await.drain().map(|(_, s)| s).collect()
    }

    /// Sends a message to every peer on the given protocol.
    pub(crate) async fn broadcast(&self, proto_id: &ProtocolID, msg: Vec<u8>) {
        for (pid, session) in self.map.read().await.iter() {
            if let Err(err) = session.send(proto_id, msg.clone()).await {
                error!("failed to send msg to {pid}: {err}");
            }
        }
    }

    /// Sends a message to the peers in `targets` on the given protocol.
    pub(crate) async fn broadcast_to(
        &self,
        proto_id: &ProtocolID,
        msg: Vec<u8>,
        targets: &HashSet<PeerID>,
    ) {
        for (pid, session) in self.map.read().await.iter() {
            if !targets.contains(pid) {
                continue;
            }
            if let Err(err) = session.send(proto_id, msg.clone()).await {
                error!("failed to send msg to {pid}: {err}");
            }
        }
    }

    /// Sends a message to one peer. Returns `PeerNotFound` if it is not in
    /// the table.
    pub(crate) async fn send_to(
        &self,
        peer_id: &PeerID,
        proto_id: &ProtocolID,
        msg: Vec<u8>,
    ) -> Result<()> {
        let map = self.map.read().await;
        let session = map
            .get(peer_id)
            .ok_or_else(|| Error::PeerNotFound(peer_id.to_string()))?;
        session.send(proto_id, msg).await
    }

    /// The protocols negotiated with a peer.
    pub(crate) async fn negotiated(&self, pid: &PeerID) -> Option<HashSet<ProtocolID>> {
        self.map
            .read()
            .await
            .get(pid)
            .map(|s| s.info.negotiated.clone())
    }

    pub(crate) async fn contains(&self, pid: &PeerID) -> bool {
        self.map.read().await.contains_key(pid)
    }

    pub(crate) async fn len(&self) -> usize {
        self.map.read().await.len()
    }

    /// Ids and endpoints of the peers that `keep` accepts.
    pub(crate) async fn peers_by(
        &self,
        keep: impl Fn(&SessionInfo) -> bool,
    ) -> HashMap<PeerID, Endpoint> {
        self.map
            .read()
            .await
            .iter()
            .filter(|(_, s)| keep(&s.info))
            .map(|(id, s)| (id.clone(), s.info.endpoint.clone()))
            .collect()
    }
}

/// Decides whether a new connection to a peer replaces the one already in
/// the pool. When two nodes dial each other at the same time, each sees
/// both connections in opposite directions. Both keep the one dialed by
/// the node with the smaller PeerID, so they keep the same connection.
/// Two connections in the same direction keep the first.
fn new_conn_wins(own: &PeerID, remote: &PeerID, old: &ConnDirection, new: &ConnDirection) -> bool {
    match (old, new) {
        // The new one was dialed by us.
        (ConnDirection::Inbound, ConnDirection::Outbound) => own.0 < remote.0,
        // The new one was dialed by the remote.
        (ConnDirection::Outbound, ConnDirection::Inbound) => remote.0 < own.0,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_nodes_keep_the_same_connection() {
        let a = PeerID([1; 32]);
        let b = PeerID([2; 32]);
        let (inb, outb) = (ConnDirection::Inbound, ConnDirection::Outbound);

        // A has the smaller id, so both keep the connection A dialed:
        // outbound on A, inbound on B. The other one never wins.
        assert!(new_conn_wins(&a, &b, &inb, &outb));
        assert!(!new_conn_wins(&a, &b, &outb, &inb));
        assert!(new_conn_wins(&b, &a, &outb, &inb));
        assert!(!new_conn_wins(&b, &a, &inb, &outb));

        // Same direction: the first one stays.
        assert!(!new_conn_wins(&a, &b, &outb, &outb));
        assert!(!new_conn_wins(&a, &b, &inb, &inb));
    }
}
