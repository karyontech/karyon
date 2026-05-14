use karyon_net::{
    codec::{Codec, LengthCodec},
    ByteBuffer,
};

use crate::{
    message::PeerNetMsg,
    util::{decode, encode},
};

/// Length-prefixed bincode codec for the peer data-plane wire.
#[derive(Clone)]
pub struct PeerNetMsgCodec {
    inner_codec: LengthCodec,
}

impl PeerNetMsgCodec {
    pub fn new() -> Self {
        Self {
            inner_codec: LengthCodec::default(),
        }
    }
}

impl Default for PeerNetMsgCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Codec<ByteBuffer> for PeerNetMsgCodec {
    type Message = PeerNetMsg;
    type Error = karyon_net::Error;

    fn encode(
        &self,
        src: &PeerNetMsg,
        dst: &mut ByteBuffer,
    ) -> std::result::Result<usize, karyon_net::Error> {
        let src = encode(src).map_err(|e| karyon_net::Error::IO(std::io::Error::other(e)))?;
        self.inner_codec.encode(&src, dst)
    }

    fn decode(
        &self,
        src: &mut ByteBuffer,
    ) -> std::result::Result<Option<(usize, PeerNetMsg)>, karyon_net::Error> {
        match self.inner_codec.decode(src)? {
            Some((n, s)) => {
                let (m, _) = decode::<PeerNetMsg>(&s)
                    .map_err(|e| karyon_net::Error::IO(std::io::Error::other(e)))?;
                Ok(Some((n, m)))
            }
            None => Ok(None),
        }
    }
}
