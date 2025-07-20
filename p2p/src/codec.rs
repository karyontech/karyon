use karyon_core::util::{decode, encode, encode_into_slice};

use karyon_net::codec::{ByteBuffer, Codec, Decoder, Encoder, LengthCodec};

use crate::{
    message::{NetMsg, RefreshMsg},
    Error, Result,
};

#[derive(Clone)]
pub struct NetMsgCodec {
    inner_codec: LengthCodec,
}

impl NetMsgCodec {
    pub fn new() -> Self {
        Self {
            inner_codec: LengthCodec::default(),
        }
    }
}

impl Codec for NetMsgCodec {
    type Message = NetMsg;
    type Error = Error;
}

impl Encoder for NetMsgCodec {
    type EnMessage = NetMsg;
    type EnError = Error;
    fn encode(&self, src: &Self::EnMessage, dst: &mut ByteBuffer) -> Result<usize> {
        let src = encode(src)?;
        Ok(self.inner_codec.encode(&src, dst)?)
    }
}

impl Decoder for NetMsgCodec {
    type DeMessage = NetMsg;
    type DeError = Error;
    fn decode(&self, src: &mut ByteBuffer) -> Result<Option<(usize, Self::DeMessage)>> {
        match self.inner_codec.decode(src)? {
            Some((n, s)) => {
                let (m, _) = decode::<Self::DeMessage>(&s)?;
                Ok(Some((n, m)))
            }
            None => Ok(None),
        }
    }
}

#[derive(Clone)]
pub struct RefreshMsgCodec {}

impl Codec for RefreshMsgCodec {
    type Message = RefreshMsg;
    type Error = Error;
}

impl Encoder for RefreshMsgCodec {
    type EnMessage = RefreshMsg;
    type EnError = Error;
    fn encode(&self, src: &Self::EnMessage, dst: &mut ByteBuffer) -> Result<usize> {
        let n = encode_into_slice(src, dst.as_mut())?;
        Ok(n)
    }
}

impl Decoder for RefreshMsgCodec {
    type DeMessage = RefreshMsg;
    type DeError = Error;
    fn decode(&self, src: &mut ByteBuffer) -> Result<Option<(usize, Self::DeMessage)>> {
        let (m, n) = decode::<Self::DeMessage>(src.as_ref())?;
        Ok(Some((n, m)))
    }
}
