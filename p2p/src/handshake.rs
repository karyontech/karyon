use std::{collections::HashMap, time::Duration};

use bincode::{Decode, Encode};
use log::trace;
use rand::{TryRng, rngs::SysRng};
use semver::VersionReq;

use karyon_core::async_runtime::Executor;

use karyon_net::{FramedReader, FramedWriter};

use crate::{
    Error, PeerID, Result, Version,
    codec::PeerNetMsgCodec,
    message::{PeerNetCmd, PeerNetMsg},
    protocol::{ProtocolFlags, ProtocolID, ProtocolMeta},
    util::decode,
    version::{VersionInt, version_match},
};

/// Negotiated protocol set from a successful handshake.
pub type NegotiatedProtocols = Vec<ProtocolID>;

/// Writer type for the handshake.
pub type HandshakeWriter = FramedWriter<PeerNetMsgCodec>;

/// A version and the versions it accepts, as sent on the wire.
#[derive(Decode, Encode, Debug, Clone)]
pub struct WireVersion {
    pub v: VersionInt,
    pub req: String,
}

impl From<&Version> for WireVersion {
    fn from(version: &Version) -> Self {
        Self {
            v: version.v.clone(),
            req: version.req.to_string(),
        }
    }
}

/// Version-exchange message. Both sides send one.
#[derive(Decode, Encode, Debug, Clone)]
pub struct VerMsg {
    pub peer_id: PeerID,
    pub version: WireVersion,
    pub protocols: HashMap<ProtocolID, WireVersion>,
    /// Random per connection. Both nonces give the connection key.
    pub nonce: u64,
}

/// Answers a Version message; `ack=false` means the sender rejected
/// the remote version/protocol mix.
#[derive(Decode, Encode, Debug, Clone)]
pub struct VerAckMsg {
    pub peer_id: PeerID,
    pub ack: bool,
}

/// Non-IO inputs to the handshake.
pub struct HandshakeParams<'a> {
    pub own_id: &'a PeerID,
    pub config_version: &'a Version,
    /// Local protocols with metadata (version + kind). Drives both
    /// version negotiation and the mandatory-subset check.
    pub protocols: &'a HashMap<ProtocolID, ProtocolMeta>,
    pub timeout_secs: u64,
    /// Runs the handshake timeouts.
    pub executor: &'a Executor,
    /// PeerID derived from the secure transport (TLS cert), if any.
    /// When `Some`, the handshake asserts the peer's claimed `vermsg.peer_id`
    /// matches it - so a peer can't claim an identity it can't prove.
    pub verified_peer_id: Option<&'a PeerID>,
}

/// The result of a successful handshake.
pub struct Handshaked {
    pub peer_id: PeerID,
    pub negotiated: NegotiatedProtocols,
    /// Same on both sides of the connection: the XOR of both nonces.
    pub conn_key: u64,
}

/// Run the handshake on split reader/writer. Returns the remote peer ID,
/// the set of protocols both sides support and the connection key.
///
/// Both sides send a VerMsg, check the remote one with the same rules,
/// then send a VerAck. So both sides end up with the same protocol set.
pub async fn handshake(
    reader: &mut FramedReader<PeerNetMsgCodec>,
    writer: &mut HandshakeWriter,
    p: &HandshakeParams<'_>,
) -> Result<Handshaked> {
    trace!("Init Handshake");

    let nonce = SysRng.try_next_u64()?;
    send_vermsg(writer, p, nonce).await?;

    let t = Duration::from_secs(p.timeout_secs);
    let msg: PeerNetMsg = p.executor.timeout(t, reader.recv_msg()).await??;
    let result = match msg.header.command {
        PeerNetCmd::Version => validate_version_msg(&msg, p),
        cmd => Err(Error::InvalidMsg(format!("unexpected msg found {cmd:?}"))),
    };

    match &result {
        Ok(_) => send_verack(writer, p.own_id, true).await?,
        Err(Error::IncompatibleVersion(_)) | Err(Error::IncompatiblePeer) => {
            send_verack(writer, p.own_id, false).await?;
        }
        _ => {}
    };
    let (pid, shared, remote_nonce) = result?;

    let msg: PeerNetMsg = p.executor.timeout(t, reader.recv_msg()).await??;
    match msg.header.command {
        PeerNetCmd::Verack => validate_verack_msg(&msg, &pid)?,
        cmd => return Err(Error::InvalidMsg(format!("unexpected msg found {cmd:?}"))),
    }

    Ok(Handshaked {
        peer_id: pid,
        negotiated: shared,
        conn_key: nonce ^ remote_nonce,
    })
}

async fn send_vermsg(
    writer: &mut HandshakeWriter,
    p: &HandshakeParams<'_>,
    nonce: u64,
) -> Result<()> {
    let protocols = p
        .protocols
        .iter()
        .map(|(k, m)| (k.clone(), WireVersion::from(&m.version)))
        .collect();

    let vermsg = VerMsg {
        peer_id: p.own_id.clone(),
        version: WireVersion::from(p.config_version),
        protocols,
        nonce,
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
/// the negotiated protocol set.
fn validate_version_msg(
    msg: &PeerNetMsg,
    p: &HandshakeParams<'_>,
) -> Result<(PeerID, NegotiatedProtocols, u64)> {
    let (vermsg, _) = decode::<VerMsg>(&msg.payload)?;

    if !mutual_match(p.config_version, &vermsg.version) {
        return Err(Error::IncompatibleVersion("system version".into()));
    }

    // Bind the claimed PeerID to the secure transport's identity. A peer
    // with a valid TLS cert for keypair X cannot claim PeerID Y.
    if let Some(vpid) = p.verified_peer_id
        && vpid != &vermsg.peer_id
    {
        return Err(Error::IncompatiblePeer);
    }

    let shared = protocols_intersection(p.protocols, &vermsg.protocols);

    if shared.is_empty() {
        return Err(Error::IncompatiblePeer);
    }

    // Every protocol the local node marked `REQUIRED` must be in the
    // negotiated intersection. Otherwise reject the handshake.
    for (id, meta) in p.protocols.iter() {
        if meta.flags.contains(ProtocolFlags::REQUIRED) && !shared.iter().any(|p| p == id) {
            return Err(Error::IncompatiblePeer);
        }
    }

    trace!("Received VerMsg from: {}", vermsg.peer_id);
    Ok((vermsg.peer_id, shared, vermsg.nonce))
}

/// Validates the given verack msg from the peer that sent `pid`'s VerMsg.
fn validate_verack_msg(msg: &PeerNetMsg, pid: &PeerID) -> Result<()> {
    let (verack, _) = decode::<VerAckMsg>(&msg.payload)?;

    if !verack.ack || &verack.peer_id != pid {
        return Err(Error::IncompatiblePeer);
    }

    trace!("Received VerAckMsg from: {}", verack.peer_id);
    Ok(())
}

/// Compute the intersection of protocols both sides support with
/// compatible versions. Returns the list of shared protocol IDs.
fn protocols_intersection(
    our_protocols: &HashMap<ProtocolID, ProtocolMeta>,
    their_protocols: &HashMap<ProtocolID, WireVersion>,
) -> NegotiatedProtocols {
    let mut shared = Vec::new();
    for (name, theirs) in their_protocols.iter() {
        if let Some(our_meta) = our_protocols.get(name)
            && mutual_match(&our_meta.version, theirs)
        {
            shared.push(name.clone());
        }
    }
    shared
}

/// True when each side accepts the other side's version. Both sides
/// run the same check, so they agree on the result.
fn mutual_match(ours: &Version, theirs: &WireVersion) -> bool {
    let Ok(their_req) = theirs.req.parse::<VersionReq>() else {
        return false;
    };
    version_match(&ours.req, &theirs.v) && version_match(&their_req, &ours.v)
}
