//! Notifications with by-position params: `[subscription, result]`.
//! Included only by the tests that use it.

use serde_json::{from_value, json};

use karyon_jsonrpc::{
    error::{Error, Result},
    message::{Notification, NotificationResult, JSONRPC_VERSION},
    server::channel::NewNotification,
};

/// Server side: `params` is `[subscription, result]`.
pub fn encode(nt: NewNotification) -> Notification {
    Notification {
        jsonrpc: JSONRPC_VERSION.to_string(),
        method: nt.method,
        params: Some(json!([nt.sub_id, nt.result])),
    }
}

/// Client side: read `[subscription, result]`.
pub fn decode(nt: Notification) -> Result<NotificationResult> {
    let params = nt
        .params
        .ok_or_else(|| Error::InvalidMsg("missing params".into()))?;
    let (subscription, result) = from_value(params)?;
    Ok(NotificationResult {
        subscription,
        result: Some(result),
    })
}
