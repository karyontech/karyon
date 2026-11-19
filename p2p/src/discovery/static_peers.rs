use std::sync::Arc;

use async_trait::async_trait;

use karyon_core::async_util::AsyncQueue;

use crate::{
    Config, Result,
    discovery::{DiscoveredPeer, Discovery, PeerConnectionEvent},
    message::PeerAddr,
    protocol::ProtocolFlags,
};

/// Default discovery of `Node::new`: hands the node `Config::dial_peers`
/// once and finds no other peers.
pub(crate) struct StaticDiscovery {
    dial_peers: Vec<PeerAddr>,
    queue: Arc<AsyncQueue<DiscoveredPeer>>,
}

impl StaticDiscovery {
    pub(crate) fn new(config: &Config) -> Arc<Self> {
        let dial_peers: Vec<PeerAddr> = config
            .dial_peers
            .iter()
            .filter_map(|ep| PeerAddr::from_endpoint(ep, 0))
            .collect();
        // Room for every dial peer, so `start` never waits.
        let queue = AsyncQueue::new(dial_peers.len().max(1));
        Arc::new(Self { dial_peers, queue })
    }
}

#[async_trait]
impl Discovery for StaticDiscovery {
    async fn start(self: Arc<Self>) -> Result<()> {
        for addr in self.dial_peers.iter() {
            let peer = DiscoveredPeer {
                peer_id: None,
                addrs: vec![addr.clone()],
                discovery_addrs: vec![],
            };
            self.queue.push(peer).await;
        }
        Ok(())
    }

    async fn shutdown(&self) {}

    async fn recv(&self) -> DiscoveredPeer {
        self.queue.recv().await
    }

    fn on_event(&self, _event: PeerConnectionEvent) {}

    fn advertise(&self, _item: &[u8], _flags: ProtocolFlags) {}

    fn find_peers_with(&self, _item: &[u8]) -> Vec<DiscoveredPeer> {
        vec![]
    }
}
