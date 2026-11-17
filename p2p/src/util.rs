use bincode::{Decode, Encode, config};

use crate::Result;

/// Max bytes one decode may claim. Same as the frame limit of
/// `LengthCodec`, so a valid message is never rejected.
const DECODE_LIMIT: usize = 4 * 1024 * 1024;

fn bincode_config() -> impl config::Config {
    // Without the limit, bincode trusts a length prefix from the wire and
    // allocates that many bytes before reading them.
    config::standard()
        .with_fixed_int_encoding()
        .with_limit::<DECODE_LIMIT>()
}

/// Encode the given type `T` into a `Vec<u8>`.
pub fn encode<T: Encode>(src: &T) -> Result<Vec<u8>> {
    Ok(bincode::encode_to_vec(src, bincode_config())?)
}

/// Decode a given type `T` from the given slice. Returns the decoded value
/// along with the number of bytes read.
pub fn decode<T: Decode<()>>(src: &[u8]) -> Result<(T, usize)> {
    let (result, bytes_read) = bincode::decode_from_slice::<T, _>(src, bincode_config())?;
    Ok((result, bytes_read))
}

#[cfg(test)]
mod tests {
    use super::*;

    use bincode::error::DecodeError;

    use crate::{
        Error,
        message::{PeerNetCmd, PeerNetMsg, PeerNetMsgHeader, ShutdownMsg},
    };

    /// Size of the length prefix of a `Vec<u8>` on the wire.
    const LEN_SIZE: usize = size_of::<u64>();

    #[test]
    fn decode_rejects_huge_length_prefix() {
        let msg = PeerNetMsg::new(PeerNetCmd::Shutdown, ShutdownMsg(0)).unwrap();
        let mut bytes = encode(&msg).unwrap();

        // The message ends with the payload: its length, then one byte.
        // Claim 2^40 bytes in the length.
        let at = bytes.len() - 1 - LEN_SIZE;
        bytes[at..at + LEN_SIZE].copy_from_slice(&(1u64 << 40).to_le_bytes());

        let res = decode::<PeerNetMsg>(&bytes);
        assert!(matches!(
            res,
            Err(Error::BincodeDecode(DecodeError::LimitExceeded))
        ));
    }

    #[test]
    fn decode_accepts_max_size_frame() {
        // Header (4 bytes) + payload length + payload fill one whole frame.
        let payload = vec![0u8; DECODE_LIMIT - 4 - LEN_SIZE];
        let msg = PeerNetMsg {
            header: PeerNetMsgHeader {
                command: PeerNetCmd::Protocol,
            },
            payload,
        };
        let bytes = encode(&msg).unwrap();
        assert_eq!(bytes.len(), DECODE_LIMIT);

        let (decoded, n) = decode::<PeerNetMsg>(&bytes).unwrap();
        assert_eq!(n, DECODE_LIMIT);
        assert_eq!(decoded.payload.len(), msg.payload.len());
    }
}
