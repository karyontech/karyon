use std::sync::Arc;

#[cfg(feature = "smol")]
use futures_rustls::rustls;

#[cfg(feature = "tokio")]
use tokio_rustls::rustls;

use rustls::{
    CertificateError, DigitallySignedStruct, DistinguishedName,
    Error::InvalidCertificate,
    SignatureScheme, SupportedCipherSuite, SupportedProtocolVersion,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, SupportedKxGroup},
    server::danger::{ClientCertVerified, ClientCertVerifier},
};

// The rustls crypto provider, picked by feature. aws-lc-rs wins if both
// are on.
#[cfg(all(feature = "aws-lc-rs", feature = "quic"))]
use rustls::crypto::aws_lc_rs::cipher_suite::TLS13_AES_128_GCM_SHA256;
#[cfg(feature = "aws-lc-rs")]
use rustls::crypto::aws_lc_rs::{
    cipher_suite::TLS13_CHACHA20_POLY1305_SHA256, default_provider, kx_group::X25519,
};
#[cfg(all(feature = "ring", not(feature = "aws-lc-rs"), feature = "quic"))]
use rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256;
#[cfg(all(feature = "ring", not(feature = "aws-lc-rs")))]
use rustls::crypto::ring::{
    cipher_suite::TLS13_CHACHA20_POLY1305_SHA256, default_provider, kx_group::X25519,
};

#[cfg(not(any(feature = "aws-lc-rs", feature = "ring")))]
compile_error!("karyon_p2p needs a rustls crypto provider: enable `aws-lc-rs` or `ring`");

use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};

use log::error;
use rcgen::PublicKeyData;

use karyon_core::crypto::KeyPair;

use crate::{
    PeerID, Result,
    peer_cert::{PEER_ID_EXT_OID, parse_cert, verify_cert, verify_cert_signature},
};

// NOTE: This code needs a comprehensive audit.

static PROTOCOL_VERSIONS: &[&SupportedProtocolVersion] = &[&rustls::version::TLS13];
static CIPHER_SUITES: &[SupportedCipherSuite] = &[TLS13_CHACHA20_POLY1305_SHA256];
/// QUIC also needs AES-128-GCM: quinn uses it to protect the initial packets.
/// ChaCha20 stays first so it is still preferred for the session.
#[cfg(feature = "quic")]
static QUIC_CIPHER_SUITES: &[SupportedCipherSuite] =
    &[TLS13_CHACHA20_POLY1305_SHA256, TLS13_AES_128_GCM_SHA256];
static KX_GROUPS: &[&dyn SupportedKxGroup] = &[X25519];
static SIGNATURE_SCHEMES: &[SignatureScheme] = &[SignatureScheme::ED25519];

/// Returns a TLS client configuration.
pub fn tls_client_config(
    key_pair: &KeyPair,
    peer_id: Option<PeerID>,
) -> Result<rustls::ClientConfig> {
    client_config(key_pair, peer_id, CIPHER_SUITES)
}

/// Returns a TLS client configuration for QUIC.
#[cfg(feature = "quic")]
pub fn quic_client_config(
    key_pair: &KeyPair,
    peer_id: Option<PeerID>,
) -> Result<rustls::ClientConfig> {
    client_config(key_pair, peer_id, QUIC_CIPHER_SUITES)
}

/// Returns a TLS server configuration.
pub fn tls_server_config(key_pair: &KeyPair) -> Result<rustls::ServerConfig> {
    server_config(key_pair, CIPHER_SUITES)
}

/// Returns a TLS server configuration for QUIC.
#[cfg(feature = "quic")]
pub fn quic_server_config(key_pair: &KeyPair) -> Result<rustls::ServerConfig> {
    server_config(key_pair, QUIC_CIPHER_SUITES)
}

fn client_config(
    key_pair: &KeyPair,
    peer_id: Option<PeerID>,
    cipher_suites: &[SupportedCipherSuite],
) -> Result<rustls::ClientConfig> {
    let (cert, private_key) = generate_cert(key_pair)?;
    let server_verifier = SrvrCertVerifier { peer_id };

    let client_config = rustls::ClientConfig::builder_with_provider(
        CryptoProvider {
            kx_groups: KX_GROUPS.to_vec(),
            cipher_suites: cipher_suites.to_vec(),
            ..default_provider()
        }
        .into(),
    )
    .with_protocol_versions(PROTOCOL_VERSIONS)?
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(server_verifier))
    .with_client_auth_cert(vec![cert], private_key)?;

    Ok(client_config)
}

fn server_config(
    key_pair: &KeyPair,
    cipher_suites: &[SupportedCipherSuite],
) -> Result<rustls::ServerConfig> {
    let (cert, private_key) = generate_cert(key_pair)?;
    let client_verifier = CliCertVerifier {};
    let server_config = rustls::ServerConfig::builder_with_provider(
        CryptoProvider {
            kx_groups: KX_GROUPS.to_vec(),
            cipher_suites: cipher_suites.to_vec(),
            ..default_provider()
        }
        .into(),
    )
    .with_protocol_versions(PROTOCOL_VERSIONS)?
    .with_client_cert_verifier(Arc::new(client_verifier))
    .with_single_cert(vec![cert], private_key)?;

    Ok(server_config)
}

/// Generates a certificate and returns both the certificate and the private key.
fn generate_cert<'a>(key_pair: &KeyPair) -> Result<(CertificateDer<'a>, PrivateKeyDer<'a>)> {
    let cert_key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
    let private_key = PrivateKeyDer::Pkcs8(cert_key_pair.serialize_der().into());

    // Add a custom extension to the certificate:
    //   - Sign the certificate's public key with the provided key pair's private key
    //   - Append both the computed signature and the key pair's public key to the extension
    let signature = key_pair.sign(&cert_key_pair.subject_public_key_info());
    let ext_content = yasna::encode_der(&(key_pair.public().as_bytes().to_vec(), signature));
    let mut ext = rcgen::CustomExtension::from_oid_content(PEER_ID_EXT_OID, ext_content);
    // XXX: Non-critical because rustls rejects unknown critical extensions
    // before our custom verifiers can run. The extension is still validated
    // by verify_cert() which requires it to be present and checks the signature.
    ext.set_criticality(false);

    let mut params = rcgen::CertificateParams::new(vec![])?;
    params.custom_extensions.push(ext);

    let cert = CertificateDer::from(params.self_signed(&cert_key_pair)?);
    Ok((cert, private_key))
}

#[derive(Debug)]
struct SrvrCertVerifier {
    peer_id: Option<PeerID>,
}

impl ServerCertVerifier for SrvrCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let peer_id = match verify_cert(end_entity) {
            Ok(pid) => pid,
            Err(err) => {
                error!("Failed to verify cert: {err}");
                return Err(err);
            }
        };

        // Verify that the peer id in the certificate's extension matches the
        // one the client intends to connect to.
        // Both should be equal for establishing a fully secure connection.
        if let Some(pid) = &self.peer_id
            && pid != &peer_id
        {
            return Err(InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ));
        }

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        unreachable!("ONLY SUPPORT tls 13 VERSION")
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        let cert = parse_cert(cert)?;
        verify_cert_signature(&cert, message, dss.signature())?;
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        SIGNATURE_SCHEMES.to_vec()
    }
}

#[derive(Debug)]
struct CliCertVerifier {}
impl ClientCertVerifier for CliCertVerifier {
    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        if let Err(err) = verify_cert(end_entity) {
            error!("Failed to verify cert: {err}");
            return Err(err);
        };
        Ok(ClientCertVerified::assertion())
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        unreachable!("ONLY SUPPORT tls 13 VERSION")
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        let cert = parse_cert(cert)?;
        verify_cert_signature(&cert, message, dss.signature())?;
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        SIGNATURE_SCHEMES.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use karyon_core::crypto::KeyPairType;

    use super::*;

    #[test]
    fn verify_generated_certificate() {
        let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
        let (cert, _) = generate_cert(&key_pair).unwrap();

        let result = verify_cert(&cert);
        assert!(result.is_ok());
        let peer_id = result.unwrap();
        assert_eq!(peer_id, PeerID::try_from(key_pair.public()).unwrap());
        assert_eq!(peer_id.0, key_pair.public().as_bytes());
    }
}
