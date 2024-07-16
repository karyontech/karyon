use std::{collections::HashMap, fmt, sync::Arc};

use async_channel::Sender;
use bincode::Encode;

use karyon_core::{
    event::{EventEmitter, EventListener},
    util::encode,
};

use karyon_net::{Conn, Endpoint};

use crate::{
    message::{NetMsg, NetMsgCmd, ProtocolMsg, ShutdownMsg},
    protocol::{Protocol, ProtocolEvent, ProtocolID},
    Error, Result,
};

/// Defines the direction of a network connection.
#[derive(Clone, Debug)]
pub enum ConnDirection {
    Inbound,
    Outbound,
}

impl fmt::Display for ConnDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConnDirection::Inbound => write!(f, "Inbound"),
            ConnDirection::Outbound => write!(f, "Outbound"),
        }
    }
}

pub struct Connection {
    pub(crate) direction: ConnDirection,
    conn: Conn<NetMsg>,
    disconnect_signal: Sender<Result<()>>,
    /// `EventEmitter` responsible for sending events to the registered protocols.
    protocol_events: Arc<EventEmitter<ProtocolID>>,
    pub(crate) remote_endpoint: Endpoint,
    listeners: HashMap<ProtocolID, EventListener<ProtocolID, ProtocolEvent>>,
}

impl Connection {
    pub fn new(
        conn: Conn<NetMsg>,
        signal: Sender<Result<()>>,
        direction: ConnDirection,
        remote_endpoint: Endpoint,
    ) -> Self {
        Self {
            conn,
            direction,
            protocol_events: EventEmitter::new(),
            disconnect_signal: signal,
            remote_endpoint,
            listeners: HashMap::new(),
        }
    }

    pub async fn send<T: Encode>(&self, protocol_id: ProtocolID, msg: T) -> Result<()> {
        let payload = encode(&msg)?;

        let proto_msg = ProtocolMsg {
            protocol_id,
            payload: payload.to_vec(),
        };

        let msg = NetMsg::new(NetMsgCmd::Protocol, &proto_msg)?;
        self.conn.send(msg).await.map_err(Error::from)
    }

    pub async fn recv<P: Protocol>(&self) -> Result<ProtocolEvent> {
        match self.listeners.get(&P::id()) {
            Some(l) => l.recv().await.map_err(Error::from),
            None => Err(Error::UnsupportedProtocol(P::id())),
        }
    }

    /// Registers a listener for the given Protocol `P`.
    pub async fn register_protocol(&mut self, protocol_id: String) {
        let listener = self.protocol_events.register(&protocol_id).await;
        self.listeners.insert(protocol_id, listener);
    }

    pub async fn emit_msg(&self, id: &ProtocolID, event: &ProtocolEvent) -> Result<()> {
        self.protocol_events.emit_by_topic(id, event).await?;
        Ok(())
    }

    pub async fn recv_inner(&self) -> Result<NetMsg> {
        self.conn.recv().await.map_err(Error::from)
    }

    pub async fn send_inner(&self, msg: NetMsg) -> Result<()> {
        self.conn.send(msg).await.map_err(Error::from)
    }

    pub async fn disconnect(&self, res: Result<()>) -> Result<()> {
        self.protocol_events.clear().await;
        self.disconnect_signal.send(res).await?;

        let m = NetMsg::new(NetMsgCmd::Shutdown, ShutdownMsg(0)).expect("Create shutdown message");
        self.conn.send(m).await.map_err(Error::from)?;

        Ok(())
    }
}
