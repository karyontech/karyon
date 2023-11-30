use log::debug;
use serde::{de::DeserializeOwned, Serialize};

use karyons_core::util::random_32;
use karyons_net::ToConn;

use crate::{
    codec::{Codec, CodecConfig},
    message, Error, Result, JSONRPC_VERSION,
};

/// Represents client config
#[derive(Default)]
pub struct ClientConfig {
    pub timeout: Option<u64>,
}

/// Represents an RPC client
pub struct Client {
    codec: Codec,
    config: ClientConfig,
}

impl Client {
    /// Creates a new RPC client by passing a Tcp, Unix, or Tls connection.
    pub fn new<C: ToConn>(conn: C, config: ClientConfig) -> Self {
        let codec_config = CodecConfig {
            max_allowed_buffer_size: 0,
            ..Default::default()
        };
        let codec = Codec::new(conn.to_conn(), codec_config);
        Self { codec, config }
    }

    /// Calls the named method, waits for the response, and returns the result.
    pub async fn call<T: Serialize + DeserializeOwned, V: DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<V> {
        let id = serde_json::json!(random_32());

        let request = message::Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method: method.to_string(),
            params: serde_json::json!(params),
        };

        let mut payload = serde_json::to_vec(&request)?;
        payload.push(b'\n');
        self.codec.write_all(&payload).await?;
        debug!("--> {request}");

        let mut buffer = vec![];
        if let Some(t) = self.config.timeout {
            self.codec.read_until_timeout(&mut buffer, t).await?;
        } else {
            self.codec.read_until(&mut buffer).await?;
        };

        let response = serde_json::from_slice::<message::Response>(&buffer)?;
        debug!("<-- {response}");

        if let Some(error) = response.error {
            return Err(Error::CallError(error.code, error.message));
        }

        if response.id.is_none() || response.id.unwrap() != request.id {
            return Err(Error::InvalidMsg("Invalid response id"));
        }

        match response.result {
            Some(result) => Ok(serde_json::from_value::<V>(result)?),
            None => Err(Error::InvalidMsg("Invalid response result")),
        }
    }
}
