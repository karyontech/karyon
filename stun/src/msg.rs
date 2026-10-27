use std::net::SocketAddr;

use rand::{rng, Rng};

use crate::attrs::Attribute;
use crate::error::{Error, Result};

/// Fixed magic cookie (RFC 5389 section 6). Also the XOR key for
/// XOR-MAPPED-ADDRESS.
pub const MAGIC_COOKIE: u32 = 0x2112_A442;

/// Fixed STUN header size in bytes.
pub const HEADER_SIZE: usize = 20;

/// 96-bit transaction id matching a response to its request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TransactionId(pub [u8; 12]);

impl TransactionId {
    pub fn random() -> Self {
        let mut b = [0u8; 12];
        rng().fill_bytes(&mut b);
        Self(b)
    }
}

/// The three message types karyon_stun speaks (Binding only).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MessageType {
    BindingRequest,
    BindingResponse,
    BindingError,
}

impl From<MessageType> for u16 {
    fn from(t: MessageType) -> u16 {
        match t {
            MessageType::BindingRequest => 0x0001,
            MessageType::BindingResponse => 0x0101,
            MessageType::BindingError => 0x0111,
        }
    }
}

impl TryFrom<u16> for MessageType {
    type Error = Error;

    fn try_from(v: u16) -> Result<Self> {
        match v {
            0x0001 => Ok(MessageType::BindingRequest),
            0x0101 => Ok(MessageType::BindingResponse),
            0x0111 => Ok(MessageType::BindingError),
            t => Err(Error::InvalidMsg(format!(
                "unsupported message type {t:#06x}"
            ))),
        }
    }
}

/// A STUN message: type, transaction id, and attributes.
#[derive(Clone, Debug)]
pub struct StunMessage {
    pub msg_type: MessageType,
    pub txn_id: TransactionId,
    pub attrs: Vec<Attribute>,
}

impl StunMessage {
    /// A Binding Request with a fresh random transaction id.
    pub fn binding_request() -> Self {
        Self {
            msg_type: MessageType::BindingRequest,
            txn_id: TransactionId::random(),
            attrs: vec![],
        }
    }

    /// The Binding Response for `request`, mapping `source`.
    pub fn binding_response(request: &StunMessage, source: SocketAddr) -> Self {
        Self {
            msg_type: MessageType::BindingResponse,
            txn_id: request.txn_id,
            attrs: vec![Attribute::XorMappedAddress(source)],
        }
    }

    /// An error response for `request`.
    pub fn error_response(request: &StunMessage, code: u16, reason: &str) -> Self {
        Self {
            msg_type: MessageType::BindingError,
            txn_id: request.txn_id,
            attrs: vec![Attribute::ErrorCode {
                code,
                reason: reason.to_string(),
            }],
        }
    }

    /// The mapped address from a response: XOR-MAPPED-ADDRESS preferred,
    /// MAPPED-ADDRESS as legacy fallback.
    pub fn mapped_address(&self) -> Result<SocketAddr> {
        for attr in &self.attrs {
            if let Attribute::XorMappedAddress(sa) = attr {
                return Ok(*sa);
            }
        }
        for attr in &self.attrs {
            if let Attribute::MappedAddress(sa) = attr {
                return Ok(*sa);
            }
        }
        Err(Error::NoMappedAddress)
    }

    /// Encode to wire bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut body = Vec::new();
        for attr in &self.attrs {
            attr.encode(&self.txn_id, &mut body)?;
        }
        if body.len() > u16::MAX as usize {
            return Err(Error::InvalidMsg("message too large".into()));
        }

        let mut out = Vec::with_capacity(HEADER_SIZE + body.len());
        out.extend_from_slice(&u16::from(self.msg_type).to_be_bytes());
        out.extend_from_slice(&(body.len() as u16).to_be_bytes());
        out.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        out.extend_from_slice(&self.txn_id.0);
        out.extend_from_slice(&body);
        Ok(out)
    }

    /// Decode from wire bytes. Returns (consumed, message), or None if
    /// `src` does not hold a complete message yet (stream transports).
    pub fn decode(src: &[u8]) -> Result<Option<(usize, Self)>> {
        if src.len() < HEADER_SIZE {
            return Ok(None);
        }

        let raw_type = u16::from_be_bytes([src[0], src[1]]);
        if raw_type & 0xC000 != 0 {
            return Err(Error::InvalidMsg("top two bits not zero".into()));
        }
        let length = u16::from_be_bytes([src[2], src[3]]) as usize;
        if !length.is_multiple_of(4) {
            return Err(Error::InvalidMsg("length not 4-byte aligned".into()));
        }
        let cookie = u32::from_be_bytes([src[4], src[5], src[6], src[7]]);
        if cookie != MAGIC_COOKIE {
            return Err(Error::InvalidMsg("bad magic cookie".into()));
        }
        if src.len() < HEADER_SIZE + length {
            return Ok(None);
        }

        let msg_type = MessageType::try_from(raw_type)?;
        let mut txn = [0u8; 12];
        txn.copy_from_slice(&src[8..HEADER_SIZE]);
        let txn_id = TransactionId(txn);

        let mut attrs = Vec::new();
        let mut off = HEADER_SIZE;
        let end = HEADER_SIZE + length;
        while off < end {
            match Attribute::decode(&txn_id, &src[off..end])? {
                Some((n, attr)) => {
                    attrs.push(attr);
                    off += n;
                }
                None => return Err(Error::InvalidMsg("truncated attribute".into())),
            }
        }

        Ok(Some((
            end,
            Self {
                msg_type,
                txn_id,
                attrs,
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_roundtrip() {
        let request = StunMessage::binding_request();
        let bytes = request.encode().unwrap();
        assert_eq!(bytes.len(), HEADER_SIZE);
        let (n, decoded) = StunMessage::decode(&bytes).unwrap().unwrap();
        assert_eq!(n, bytes.len());
        assert_eq!(decoded.msg_type, MessageType::BindingRequest);
        assert_eq!(decoded.txn_id, request.txn_id);

        let source: SocketAddr = "203.0.113.9:54321".parse().unwrap();
        let resp = StunMessage::binding_response(&request, source);
        let bytes = resp.encode().unwrap();
        let (_, decoded) = StunMessage::decode(&bytes).unwrap().unwrap();
        assert_eq!(decoded.msg_type, MessageType::BindingResponse);
        assert_eq!(decoded.txn_id, request.txn_id);
        assert_eq!(decoded.mapped_address().unwrap(), source);
    }

    #[test]
    fn error_response_roundtrip() {
        let request = StunMessage::binding_request();
        let resp = StunMessage::error_response(&request, 400, "Bad Request");
        let bytes = resp.encode().unwrap();
        let (_, decoded) = StunMessage::decode(&bytes).unwrap().unwrap();
        assert_eq!(decoded.msg_type, MessageType::BindingError);
        assert!(matches!(
            &decoded.attrs[0],
            Attribute::ErrorCode { code: 400, reason } if reason == "Bad Request"
        ));
    }

    #[test]
    fn decode_incomplete_returns_none() {
        let request = StunMessage::binding_request();
        let bytes = StunMessage::binding_response(&request, "10.0.0.1:80".parse().unwrap())
            .encode()
            .unwrap();
        for cut in 0..bytes.len() {
            assert!(StunMessage::decode(&bytes[..cut]).unwrap().is_none());
        }
    }

    #[test]
    fn decode_rejects_garbage() {
        // Wrong magic cookie.
        let mut bytes = StunMessage::binding_request().encode().unwrap();
        bytes[4] = 0;
        assert!(StunMessage::decode(&bytes).is_err());

        // Top bits set.
        let mut bytes = StunMessage::binding_request().encode().unwrap();
        bytes[0] = 0xC0;
        assert!(StunMessage::decode(&bytes).is_err());
    }
}
