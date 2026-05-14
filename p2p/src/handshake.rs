use std::{collections::HashMap, time::Duration};

use bincode::{Decode, Encode};
use log::trace;

use karyon_core::async_util::timeout;

use karyon_net::FramedReader;

use crate::{
    codec::PeerNetMsgCodec,
    message::{PeerNetCmd, PeerNetMsg},
    protocol::{ProtocolID, ProtocolKind, ProtocolMeta},
    util::decode,
    version::{version_match, VersionInt},
    Error, PeerID, Result, Version,
};

/// Negotiated protocol set from a successful handshake.
pub type NegotiatedProtocols = Vec<ProtocolID>;

/// Writer type for the handshake.
pub type HandshakeWriter = karyon_net::FramedWriter<PeerNetMsgCodec>;

/// Version-exchange message kicked off by the outbound side.
#[derive(Decode, Encode, Debug, Clone)]
pub struct VerMsg {
    pub peer_id: PeerID,
    pub version: VersionInt,
    pub protocols: HashMap<ProtocolID, VersionInt>,
}

/// Acknowledges a Version message; `ack=false` means the responder
/// rejected the proposed version/protocol mix.
#[derive(Decode, Encode, Debug, Clone)]
pub struct VerAckMsg {
    pub peer_id: PeerID,
    pub ack: bool,
}

/// Non-IO inputs to the handshake.
pub struct HandshakeParams<'a> {
    pub own_id: &'a PeerID,
    pub is_inbound: bool,
    pub config_version: &'a Version,
    /// Local protocols with metadata (version + kind). Drives both
    /// version negotiation and the mandatory-subset check.
    pub protocols: &'a HashMap<ProtocolID, ProtocolMeta>,
    pub timeout_secs: u64,
    /// PeerID derived from the secure transport (TLS cert), if any.
    /// When `Some`, the handshake asserts the peer's claimed `vermsg.peer_id`
    /// matches it - so a peer can't claim an identity it can't prove.
    pub verified_peer_id: Option<&'a PeerID>,
}

/// Run the handshake on split reader/writer. Returns the remote peer ID
/// and the set of protocols both sides support.
pub async fn handshake(
    reader: &mut FramedReader<PeerNetMsgCodec>,
    writer: &mut HandshakeWriter,
    p: &HandshakeParams<'_>,
) -> Result<(PeerID, NegotiatedProtocols)> {
    trace!("Init Handshake");

    if !p.is_inbound {
        send_vermsg(writer, p.own_id, p.config_version, p.protocols).await?;
    }

    let t = Duration::from_secs(p.timeout_secs);
    let msg: PeerNetMsg = timeout(t, reader.recv_msg()).await??;

    match msg.header.command {
        PeerNetCmd::Version => {
            let result =
                validate_version_msg(&msg, p.config_version, p.protocols, p.verified_peer_id).await;
            match &result {
                Ok(_) => send_verack(writer, p.own_id, true).await?,
                Err(Error::IncompatibleVersion(_)) | Err(Error::IncompatiblePeer) => {
                    send_verack(writer, p.own_id, false).await?;
                }
                _ => {}
            };
            result
        }
        PeerNetCmd::Verack => {
            let pid = validate_verack_msg(&msg, p.verified_peer_id).await?;
            // Outbound side: we sent VerMsg, got VerAck. We don't know
            // the intersection yet - return all our protocols. The inbound
            // side already computed the intersection and will only run
            // the shared set.
            let all_protos = p.protocols.keys().cloned().collect();
            Ok((pid, all_protos))
        }
        cmd => Err(Error::InvalidMsg(format!("unexpected msg found {cmd:?}"))),
    }
}

/// Sends a Version message.
async fn send_vermsg(
    writer: &mut HandshakeWriter,
    own_id: &PeerID,
    config_version: &Version,
    protocols: &HashMap<ProtocolID, ProtocolMeta>,
) -> Result<()> {
    let proto_versions = protocols
        .iter()
        .map(|(k, m)| (k.clone(), m.version.v.clone()))
        .collect();

    let vermsg = VerMsg {
        peer_id: own_id.clone(),
        protocols: proto_versions,
        version: config_version.v.clone(),
    };

    trace!("Send VerMsg");
    writer
        .send_msg(PeerNetMsg::new(PeerNetCmd::Version, &vermsg)?)
        .await?;
    Ok(())
}

/// Sends a Verack message.
async fn send_verack(writer: &mut HandshakeWriter, own_id: &PeerID, ack: bool) -> Result<()> {
    let verack = VerAckMsg {
        peer_id: own_id.clone(),
        ack,
    };

    trace!("Send VerAckMsg {verack:?}");
    writer
        .send_msg(PeerNetMsg::new(PeerNetCmd::Verack, &verack)?)
        .await?;
    Ok(())
}

/// Validates the given version msg. Returns the remote peer ID and
/// the intersection of compatible protocols.
async fn validate_version_msg(
    msg: &PeerNetMsg,
    config_version: &Version,
    protocols: &HashMap<ProtocolID, ProtocolMeta>,
    verified_peer_id: Option<&PeerID>,
) -> Result<(PeerID, NegotiatedProtocols)> {
    let (vermsg, _) = decode::<VerMsg>(&msg.payload)?;

    if !version_match(&config_version.req, &vermsg.version) {
        return Err(Error::IncompatibleVersion("system version".into()));
    }

    // Bind the claimed PeerID to the secure transport's identity. A peer
    // with a valid TLS cert for keypair X cannot claim PeerID Y.
    if let Some(vpid) = verified_peer_id {
        if vpid != &vermsg.peer_id {
            return Err(Error::IncompatiblePeer);
        }
    }

    let shared = protocols_intersection(protocols, &vermsg.protocols);

    if shared.is_empty() {
        return Err(Error::IncompatiblePeer);
    }

    // Every protocol the local node marked `Mandatory` must be in the
    // negotiated intersection. Otherwise reject the handshake.
    for (id, meta) in protocols.iter() {
        if matches!(meta.kind, ProtocolKind::Mandatory) && !shared.iter().any(|p| p == id) {
            return Err(Error::IncompatiblePeer);
        }
    }

    trace!("Received VerMsg from: {}", vermsg.peer_id);
    Ok((vermsg.peer_id, shared))
}

/// Validates the given verack msg.
async fn validate_verack_msg(
    msg: &PeerNetMsg,
    verified_peer_id: Option<&PeerID>,
) -> Result<PeerID> {
    let (verack, _) = decode::<VerAckMsg>(&msg.payload)?;

    if !verack.ack {
        return Err(Error::IncompatiblePeer);
    }

    if let Some(vpid) = verified_peer_id {
        if vpid != &verack.peer_id {
            return Err(Error::IncompatiblePeer);
        }
    }

    trace!("Received VerAckMsg from: {}", verack.peer_id);
    Ok(verack.peer_id)
}

/// Compute the intersection of protocols both sides support with
/// compatible versions. Returns the list of shared protocol IDs.
fn protocols_intersection(
    our_protocols: &HashMap<ProtocolID, ProtocolMeta>,
    their_protocols: &HashMap<String, VersionInt>,
) -> NegotiatedProtocols {
    let mut shared = Vec::new();
    for (name, their_version) in their_protocols.iter() {
        if let Some(our_meta) = our_protocols.get(name) {
            if version_match(&our_meta.version.req, their_version) {
                shared.push(name.clone());
            }
        }
    }
    shared
}
