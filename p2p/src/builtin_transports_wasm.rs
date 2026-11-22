//! wasm has no built-in transports: only `Config::transport` dials and
//! listens. Takes the place of `builtin_transports.rs`.

use karyon_core::crypto::KeyPair;
use karyon_net::Endpoint;

use crate::{Error, PeerID, Result, connector::DialResult, listener::StreamListener};

pub(crate) struct BuiltinTransports;

impl BuiltinTransports {
    pub(crate) fn new(_key_pair: &KeyPair) -> Self {
        Self
    }

    pub(crate) async fn dial(
        &self,
        endpoint: &Endpoint,
        _peer_id: &Option<PeerID>,
    ) -> Result<DialResult> {
        Err(Error::UnsupportedEndpoint(endpoint.to_string()))
    }

    pub(crate) async fn listen(&self, endpoint: &Endpoint) -> Result<StreamListener> {
        Err(Error::UnsupportedEndpoint(endpoint.to_string()))
    }
}
