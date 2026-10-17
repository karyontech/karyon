use karyon_net::codec::Codec;
use karyon_net::ByteBuffer;

use crate::error::Error;
use crate::msg::StunMessage;

/// karyon_net codec for STUN over byte streams (TCP). UDP uses
/// `StunMessage::encode`/`decode` on whole datagrams directly.
#[derive(Clone, Default)]
pub struct StunCodec;

impl Codec<ByteBuffer> for StunCodec {
    type Message = StunMessage;
    type Error = Error;

    fn encode(&self, src: &StunMessage, dst: &mut ByteBuffer) -> Result<usize, Error> {
        let bytes = src.encode()?;
        dst.extend_from_slice(&bytes);
        Ok(bytes.len())
    }

    fn decode(&self, src: &mut ByteBuffer) -> Result<Option<(usize, StunMessage)>, Error> {
        StunMessage::decode(src.as_ref())
    }
}
