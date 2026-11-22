use std::collections::HashSet;

use log::warn;

use crate::{
    Error, PeerID,
    access_control::{AccessControlRef, Action, PeerCandidate, Subject},
    handshake::{HandshakeParams, Handshaked, handshake},
    protocol::{ProtocolID, ProtocolRegistry, ResolvedProtocol},
    queued_conn::QueuedConn,
    session::{SessionInfo, SessionInfoRef},
    version::Version,
};

/// The settings admission reads.
pub(crate) struct AdmitConfig {
    /// Network version, checked in the handshake.
    pub version: Version,
    /// Max time for the handshake and for the wire setup, in seconds.
    pub handshake_timeout: u64,
    pub access_control: AccessControlRef,
}

/// A connection that passed the handshake and access control, ready to
/// become a `PeerSession`.
pub(crate) struct Admitted {
    pub info: SessionInfoRef,
    pub conn: QueuedConn,
    /// Constructors of the negotiated protocols.
    pub protocols: Vec<ResolvedProtocol>,
    /// Same on both nodes for this connection (see `Handshaked`).
    pub conn_key: u64,
}

/// Why a connection was not admitted.
pub(crate) struct Rejected {
    /// The remote's id, if it is known.
    pub peer_id: Option<PeerID>,
    pub error: Error,
}

/// Runs the handshake and access control on a new connection. The peer
/// session is not built yet, so a rejected one never gets tasks or
/// streams.
pub(crate) async fn admit(
    own_id: &PeerID,
    mut conn: QueuedConn,
    registry: &ProtocolRegistry,
    config: &AdmitConfig,
) -> Result<Admitted, Rejected> {
    // Snapshot the protocol metadata so we don't hold a lock across the
    // handshake. Drives both version negotiation and the
    // mandatory-subset check.
    let meta = registry.meta().await;

    let params = HandshakeParams {
        own_id,
        config_version: &config.version,
        protocols: &meta,
        timeout_secs: config.handshake_timeout,
        verified_peer_id: conn.verified_peer_id.as_ref(),
    };
    let Handshaked {
        peer_id: pid,
        negotiated,
        conn_key,
    } = handshake(&mut conn.reader, &mut conn.writer, &params)
        .await
        .map_err(|error| Rejected {
            // On TCP only the dialed id is known; discovery needs it to
            // mark the peer failed.
            peer_id: conn
                .verified_peer_id
                .clone()
                .or_else(|| conn.dialed_peer_id.clone()),
            error,
        })?;

    let candidate = PeerCandidate {
        peer_id: &pid,
        remote_endpoint: &conn.remote_endpoint,
        negotiated_protocols: &negotiated,
    };
    let allowed = config.access_control.allow(
        &Subject::Peer(&candidate),
        Action::Connect(conn.direction.clone()),
    );
    if !allowed {
        warn!("Access denied for peer {pid}");
        return Err(Rejected {
            peer_id: Some(pid),
            error: Error::AccessDenied,
        });
    }

    let negotiated: HashSet<ProtocolID> = negotiated.into_iter().collect();
    let protocols = registry.resolve(&negotiated).await;
    let info = SessionInfoRef::new(SessionInfo {
        id: pid,
        direction: conn.direction.clone(),
        endpoint: conn.remote_endpoint.clone(),
        negotiated,
    });

    Ok(Admitted {
        info,
        conn,
        protocols,
        conn_key,
    })
}
