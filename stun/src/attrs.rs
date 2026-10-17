use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::error::{Error, Result};
use crate::msg::{TransactionId, MAGIC_COOKIE};

pub const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
pub const ATTR_ERROR_CODE: u16 = 0x0009;
pub const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
pub const ATTR_SOFTWARE: u16 = 0x8022;

const FAMILY_IPV4: u8 = 0x01;
const FAMILY_IPV6: u8 = 0x02;

/// ERROR-CODE class bits.
const CLASS_MASK: u8 = 0x07;

/// STUN attribute (RFC 5389 section 15). Unknown attributes are kept
/// as raw bytes and ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attribute {
    MappedAddress(SocketAddr),
    XorMappedAddress(SocketAddr),
    ErrorCode { code: u16, reason: String },
    Software(String),
    Unknown { attr_type: u16, value: Vec<u8> },
}

impl Attribute {
    /// Encode as TLV (type, length, value, zero padding to 4 bytes).
    pub fn encode(&self, txn_id: &TransactionId, dst: &mut Vec<u8>) -> Result<()> {
        let (attr_type, value) = match self {
            Attribute::MappedAddress(sa) => (ATTR_MAPPED_ADDRESS, encode_address(sa)),
            Attribute::XorMappedAddress(sa) => (
                ATTR_XOR_MAPPED_ADDRESS,
                encode_address(&xor_address(sa, txn_id)),
            ),
            Attribute::ErrorCode { code, reason } => (ATTR_ERROR_CODE, encode_error(*code, reason)),
            Attribute::Software(s) => (ATTR_SOFTWARE, s.as_bytes().to_vec()),
            Attribute::Unknown { attr_type, value } => (*attr_type, value.clone()),
        };

        if value.len() > u16::MAX as usize {
            return Err(Error::InvalidMsg("attribute value too large".into()));
        }

        dst.extend_from_slice(&attr_type.to_be_bytes());
        dst.extend_from_slice(&(value.len() as u16).to_be_bytes());
        dst.extend_from_slice(&value);
        // Pad to a 4-byte boundary.
        for _ in 0..(4 - value.len() % 4) % 4 {
            dst.push(0);
        }
        Ok(())
    }

    /// Decode one attribute. Returns (consumed bytes incl. padding, attr),
    /// or None if `src` does not hold a complete attribute.
    pub fn decode(txn_id: &TransactionId, src: &[u8]) -> Result<Option<(usize, Self)>> {
        if src.len() < 4 {
            return Ok(None);
        }
        let attr_type = u16::from_be_bytes([src[0], src[1]]);
        let len = u16::from_be_bytes([src[2], src[3]]) as usize;
        let padded = (len + 3) & !3;
        if src.len() < 4 + padded {
            return Ok(None);
        }
        let value = &src[4..4 + len];

        let attr = match attr_type {
            ATTR_MAPPED_ADDRESS => Attribute::MappedAddress(decode_address(value)?),
            // XOR is an involution: applying it to the wire form
            // recovers the real address.
            ATTR_XOR_MAPPED_ADDRESS => {
                Attribute::XorMappedAddress(xor_address(&decode_address(value)?, txn_id))
            }
            ATTR_ERROR_CODE => decode_error(value)?,
            ATTR_SOFTWARE => Attribute::Software(String::from_utf8_lossy(value).into_owned()),
            t => Attribute::Unknown {
                attr_type: t,
                value: value.to_vec(),
            },
        };
        Ok(Some((4 + padded, attr)))
    }
}

/// XOR an address with the magic cookie (and transaction id for IPv6),
/// per RFC 5389 section 15.2. Involution: applying twice is identity.
fn xor_address(sa: &SocketAddr, txn_id: &TransactionId) -> SocketAddr {
    let port = sa.port() ^ (MAGIC_COOKIE >> 16) as u16;
    let ip = match sa.ip() {
        IpAddr::V4(v4) => {
            let mut o = v4.octets();
            for (b, k) in o.iter_mut().zip(MAGIC_COOKIE.to_be_bytes()) {
                *b ^= k;
            }
            IpAddr::V4(Ipv4Addr::from(o))
        }
        IpAddr::V6(v6) => {
            let mut key = [0u8; 16];
            key[..4].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
            key[4..].copy_from_slice(&txn_id.0);
            let mut o = v6.octets();
            for (b, k) in o.iter_mut().zip(key) {
                *b ^= k;
            }
            IpAddr::V6(Ipv6Addr::from(o))
        }
    };
    SocketAddr::new(ip, port)
}

/// Address value: [0, family, port (2 bytes), address (4 or 16 bytes)].
fn encode_address(sa: &SocketAddr) -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    v.push(0);
    match sa.ip() {
        IpAddr::V4(ip) => {
            v.push(FAMILY_IPV4);
            v.extend_from_slice(&sa.port().to_be_bytes());
            v.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            v.push(FAMILY_IPV6);
            v.extend_from_slice(&sa.port().to_be_bytes());
            v.extend_from_slice(&ip.octets());
        }
    }
    v
}

fn decode_address(value: &[u8]) -> Result<SocketAddr> {
    if value.len() < 4 {
        return Err(Error::InvalidMsg("address attribute too short".into()));
    }
    let port = u16::from_be_bytes([value[2], value[3]]);
    let ip = match value[1] {
        FAMILY_IPV4 if value.len() == 8 => {
            let o: [u8; 4] = value[4..8].try_into().expect("length checked");
            IpAddr::V4(Ipv4Addr::from(o))
        }
        FAMILY_IPV6 if value.len() == 20 => {
            let o: [u8; 16] = value[4..20].try_into().expect("length checked");
            IpAddr::V6(Ipv6Addr::from(o))
        }
        _ => return Err(Error::InvalidMsg("bad address family or length".into())),
    };
    Ok(SocketAddr::new(ip, port))
}

/// Error value: [0, 0, class, number] + utf8 reason. Class is the
/// low 3 bits of its byte; the rest are reserved (RFC 5389 15.6).
fn encode_error(code: u16, reason: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(4 + reason.len());
    let class = (code / 100) as u8 & CLASS_MASK;
    v.extend_from_slice(&[0, 0, class, (code % 100) as u8]);
    v.extend_from_slice(reason.as_bytes());
    v
}

fn decode_error(value: &[u8]) -> Result<Attribute> {
    if value.len() < 4 {
        return Err(Error::InvalidMsg("error attribute too short".into()));
    }
    let code = (value[2] & CLASS_MASK) as u16 * 100 + value[3] as u16;
    let reason = String::from_utf8_lossy(&value[4..]).into_owned();
    Ok(Attribute::ErrorCode { code, reason })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn txn() -> TransactionId {
        TransactionId([7u8; 12])
    }

    #[test]
    fn xor_is_involution() {
        let addrs = ["192.168.1.1:8080", "[2001:db8::1]:3478"];
        for a in addrs {
            let sa: SocketAddr = a.parse().unwrap();
            assert_eq!(xor_address(&xor_address(&sa, &txn()), &txn()), sa);
        }
    }

    #[test]
    fn xor_port_uses_cookie_high_bits() {
        let sa: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let x = xor_address(&sa, &txn());
        assert_eq!(x.port(), 8080 ^ 0x2112);
    }

    #[test]
    fn error_code_ignores_reserved_bits() {
        // Class byte 0x84: reserved bits set, class 4.
        let value = [0, 0, 0x84, 0, b'x'];
        let attr = decode_error(&value).unwrap();
        assert!(matches!(attr, Attribute::ErrorCode { code: 400, .. }));
    }

    #[test]
    fn attribute_roundtrip() {
        let attrs = vec![
            Attribute::MappedAddress("10.0.0.1:1234".parse().unwrap()),
            Attribute::XorMappedAddress("[::1]:3478".parse().unwrap()),
            Attribute::ErrorCode {
                code: 400,
                reason: "Bad Request".into(),
            },
            Attribute::Software("karyon".into()),
        ];
        for attr in attrs {
            let mut buf = Vec::new();
            attr.encode(&txn(), &mut buf).unwrap();
            assert_eq!(buf.len() % 4, 0);
            let (n, decoded) = Attribute::decode(&txn(), &buf).unwrap().unwrap();
            assert_eq!(n, buf.len());
            assert_eq!(decoded, attr);
        }
    }
}
