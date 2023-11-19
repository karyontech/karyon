use log::debug;
use serde::{de::DeserializeOwned, Serialize};

use karyons_core::{async_utils::timeout, utils::random_32};
use karyons_net::{dial, Conn, Endpoint};

use crate::{
    message,
    utils::{read_until, write_all},
    Error, Result, JSONRPC_VERSION,
};

/// Represents an RPC client
pub struct Client {
    conn: Conn,
    timeout: Option<u64>,
}

impl Client {
    /// Creates a new RPC client.
    pub fn new(conn: Conn, timeout: Option<u64>) -> Self {
        Self { conn, timeout }
    }

    /// Creates a new RPC client using the provided endpoint.
    pub async fn new_with_endpoint(endpoint: &Endpoint, timeout: Option<u64>) -> Result<Self> {
        let conn = dial(endpoint).await?;
        Ok(Self { conn, timeout })
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

        let payload = serde_json::to_vec(&request)?;
        write_all(&self.conn, &payload).await?;
        debug!("--> {request}");

        let mut buffer = vec![];
        if let Some(t) = self.timeout {
            timeout(
                std::time::Duration::from_secs(t),
                read_until(&self.conn, &mut buffer),
            )
            .await?
        } else {
            read_until(&self.conn, &mut buffer).await
        }?;

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
