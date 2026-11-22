//! The built-in TCP, TLS and QUIC transports. wasm has none: there
//! `builtin_transports_wasm.rs` takes this module's place.

use karyon_core::crypto::KeyPair;
use karyon_net::{
    ClientLayer, Endpoint,
    tcp::{self, TcpListener},
    tls::{ClientTlsConfig, ServerTlsConfig, TlsLayer},
};

use crate::{
    Error, PeerID, Result,
    connector::DialResult,
    listener::StreamListener,
    peer_cert::peer_id_from_certs,
    tls_config::{tls_client_config, tls_server_config},
};

#[cfg(feature = "quic")]
use karyon_core::async_runtime::Executor;
#[cfg(feature = "quic")]
use karyon_net::{
    StreamMux,
    quic::{ClientQuicConfig, QuicEndpoint, QuicRuntimeRef, ServerQuicConfig},
};

#[cfg(feature = "quic")]
use crate::tls_config::{quic_client_config, quic_server_config};

static DNS_NAME: &str = "karyontech.net";

/// Dials and listens with the built-in TCP, TLS and QUIC.
pub(crate) struct BuiltinTransports {
    /// Signs the TLS and QUIC certificates.
    key_pair: KeyPair,
    /// Runs the built-in QUIC (see `Config::quic_runtime`).
    #[cfg(feature = "quic")]
    quic_runtime: Option<QuicRuntimeRef>,
}

impl BuiltinTransports {
    pub(crate) fn new(
        key_pair: &KeyPair,
        #[cfg(feature = "quic")] quic_runtime: Option<QuicRuntimeRef>,
    ) -> Self {
        Self {
            key_pair: key_pair.clone(),
            #[cfg(feature = "quic")]
            quic_runtime,
        }
    }

    /// Dials a TCP or TLS endpoint.
    pub(crate) async fn dial(
        &self,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
    ) -> Result<DialResult> {
        match endpoint {
            Endpoint::Tcp(..) => {
                let stream = tcp::connect(endpoint, Default::default()).await?;
                Ok(DialResult::Channel(stream, None))
            }
            Endpoint::Tls(..) => {
                let tls_config = ClientTlsConfig {
                    client_config: tls_client_config(&self.key_pair, peer_id.clone())?,
                    dns_name: DNS_NAME.to_string(),
                };
                let stream = tcp::connect(endpoint, Default::default()).await?;
                let tls_layer = TlsLayer::client(tls_config);
                let stream = ClientLayer::handshake(&tls_layer, stream).await?;
                let vpid = stream
                    .peer_certificates()
                    .as_deref()
                    .and_then(peer_id_from_certs);
                Ok(DialResult::Channel(stream, vpid))
            }
            _ => Err(Error::UnsupportedEndpoint(endpoint.to_string())),
        }
    }

    /// Dials a QUIC endpoint and opens the first stream, for the handshake.
    #[cfg(feature = "quic")]
    pub(crate) async fn dial_quic(
        &self,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
        ex: &Executor,
    ) -> Result<DialResult> {
        let rustls_config = quic_client_config(&self.key_pair, peer_id.clone())?;
        let mut config =
            ClientQuicConfig::from_rustls(rustls_config, DNS_NAME).with_executor(ex.clone());
        if let Some(runtime) = &self.quic_runtime {
            config = config.with_runtime(runtime.clone());
        }
        let quic_conn = QuicEndpoint::dial(endpoint, config).await?;
        let vpid = quic_conn
            .peer_certificates()
            .as_deref()
            .and_then(peer_id_from_certs);
        let stream = quic_conn.open_stream().await?;
        Ok(DialResult::Quic(stream, quic_conn, vpid))
    }

    /// Listens on a TCP or TLS endpoint.
    pub(crate) async fn listen(&self, endpoint: &Endpoint) -> Result<StreamListener> {
        match endpoint {
            Endpoint::Tcp(..) => {
                let listener = TcpListener::bind(endpoint, Default::default()).await?;
                Ok(StreamListener::plain(Box::new(listener)))
            }
            Endpoint::Tls(..) => {
                let tls_config = ServerTlsConfig {
                    server_config: tls_server_config(&self.key_pair)?,
                };
                let listener = TcpListener::bind(endpoint, Default::default()).await?;
                Ok(StreamListener {
                    inner: Box::new(listener),
                    tls: Some(Box::new(TlsLayer::server(tls_config))),
                })
            }
            _ => Err(Error::UnsupportedEndpoint(endpoint.to_string())),
        }
    }

    /// The config of a built-in QUIC listener.
    #[cfg(feature = "quic")]
    pub(crate) fn quic_server_config(&self, ex: &Executor) -> Result<ServerQuicConfig> {
        let rustls_config = quic_server_config(&self.key_pair)?;
        let mut config = ServerQuicConfig::from_rustls(rustls_config).with_executor(ex.clone());
        if let Some(runtime) = &self.quic_runtime {
            config = config.with_runtime(runtime.clone());
        }
        Ok(config)
    }
}
