use karyon_core::util::{decode, encode, encode_into_slice};

use karyon_net::{
    codec::{Codec, Decoder, Encoder, LengthCodec},
    Result,
};

use crate::message::{NetMsg, RefreshMsg};

#[derive(Clone)]
pub struct NetMsgCodec {
    inner_codec: LengthCodec,
}

impl NetMsgCodec {
    pub fn new() -> Self {
        Self {
            inner_codec: LengthCodec {},
        }
    }
}

impl Codec for NetMsgCodec {
    type Item = NetMsg;
}

impl Encoder for NetMsgCodec {
    type EnItem = NetMsg;
    fn encode(&self, src: &Self::EnItem, dst: &mut [u8]) -> Result<usize> {
        let src = encode(src)?;
        self.inner_codec.encode(&src, dst)
    }
}

impl Decoder for NetMsgCodec {
    type DeItem = NetMsg;
    fn decode(&self, src: &mut [u8]) -> Result<Option<(usize, Self::DeItem)>> {
        match self.inner_codec.decode(src)? {
            Some((n, s)) => {
                let (m, _) = decode::<Self::DeItem>(&s)?;
                Ok(Some((n, m)))
            }
            None => Ok(None),
        }
    }
}

#[derive(Clone)]
pub struct RefreshMsgCodec {}

impl Codec for RefreshMsgCodec {
    type Item = RefreshMsg;
}

impl Encoder for RefreshMsgCodec {
    type EnItem = RefreshMsg;
    fn encode(&self, src: &Self::EnItem, dst: &mut [u8]) -> Result<usize> {
        let n = encode_into_slice(src, dst)?;
        Ok(n)
    }
}

impl Decoder for RefreshMsgCodec {
    type DeItem = RefreshMsg;
    fn decode(&self, src: &mut [u8]) -> Result<Option<(usize, Self::DeItem)>> {
        let (m, n) = decode::<Self::DeItem>(src)?;
        Ok(Some((n, m)))
    }
}
