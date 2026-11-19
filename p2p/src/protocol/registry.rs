use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use karyon_core::async_runtime::lock::RwLock;

use crate::protocol::{ProtocolConstructorRef, ProtocolID, ProtocolMeta};

/// `Arc` reference to the `ProtocolRegistry`.
pub(crate) type ProtocolRegistryRef = Arc<ProtocolRegistry>;

/// A protocol constructor with the id it was registered under.
pub(crate) type ResolvedProtocol = (ProtocolID, ProtocolConstructorRef);

/// The attached protocols: constructor and metadata per protocol id,
/// under one lock. The Node registers protocols, the PeerPool reads them.
pub(crate) struct ProtocolRegistry {
    entries: RwLock<HashMap<ProtocolID, (ProtocolConstructorRef, ProtocolMeta)>>,
}

impl ProtocolRegistry {
    pub(crate) fn new() -> ProtocolRegistryRef {
        Arc::new(Self {
            entries: RwLock::new(HashMap::new()),
        })
    }

    /// Adds a protocol, or replaces the one with the same id.
    pub(crate) async fn insert(
        &self,
        id: ProtocolID,
        ctor: ProtocolConstructorRef,
        meta: ProtocolMeta,
    ) {
        self.entries.write().await.insert(id, (ctor, meta));
    }

    /// Snapshot of the metadata, for the handshake.
    pub(crate) async fn meta(&self) -> HashMap<ProtocolID, ProtocolMeta> {
        let entries = self.entries.read().await;
        entries
            .iter()
            .map(|(id, (_, meta))| (id.clone(), meta.clone()))
            .collect()
    }

    /// Constructors of the negotiated protocols.
    pub(crate) async fn resolve(&self, negotiated: &HashSet<ProtocolID>) -> Vec<ResolvedProtocol> {
        let entries = self.entries.read().await;
        entries
            .iter()
            .filter(|(id, _)| negotiated.contains(*id))
            .map(|(id, (ctor, _))| (id.clone(), ctor.clone()))
            .collect()
    }
}
