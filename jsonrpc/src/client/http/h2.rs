//! HTTP/1.1 + HTTP/2 client over tokio with connection pooling.
//!
//! `hyper_util::client::legacy::Client` covers both versions
//! transparently and handles DNS plus connection reuse, so this path
//! is preferred whenever the runtime is tokio. The smol path falls
//! back to plain HTTP/1.1 because hyper's high-level Client requires
//! a tokio runtime.

use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use hyper_util::client::legacy::{connect::HttpConnector, Client as HyperClient};

use karyon_net::Endpoint;

use crate::{
    client::http::parse_response,
    error::{Error, Result},
    message,
};

pub(super) async fn send(
    client: &HyperClient<HttpConnector, Full<Bytes>>,
    endpoint: &Endpoint,
    msg: serde_json::Value,
) -> Result<message::Response> {
    let body = serde_json::to_vec(&msg)?;

    let req = Request::post(endpoint.to_string())
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .map_err(|e| Error::HttpError(e.to_string()))?;

    let response = client
        .request(req)
        .await
        .map_err(|e| Error::HttpError(e.to_string()))?;

    parse_response(response).await
}
