// TODO: AUDIT REVIEW. TLS config, cert generation and verifiers need a
// security audit before relying on them.

use std::{result::Result as StdResult, sync::Arc};

#[cfg(feature = "smol")]
use futures_rustls::rustls;

#[cfg(feature = "tokio")]
use tokio_rustls::rustls;

use rustls::{
    CertificateError, CipherSuite, ClientConfig, DigitallySignedStruct, DistinguishedName,
    Error as TlsError,
    Error::{General, InvalidCertificate},
    NamedGroup, ServerConfig, SignatureScheme, SupportedProtocolVersion,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::CryptoProvider,
    server::danger::{ClientCertVerified, ClientCertVerifier},
    version::TLS13,
};

// The default rustls crypto provider, picked by feature. aws-lc-rs wins if
// both are on.
#[cfg(feature = "aws-lc-rs")]
use rustls::crypto::aws_lc_rs::default_provider;
#[cfg(all(feature = "ring", not(feature = "aws-lc-rs")))]
use rustls::crypto::ring::default_provider;

use rcgen::{
    CertificateParams, CustomExtension, Error as RcgenError, PKCS_ED25519, PublicKeyData,
    SerialNumber, SignatureAlgorithm, SigningKey,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use sha2::{Digest, Sha256};

use log::error;

use karyon_core::crypto::{KeyPair, KeyPairType};

use crate::{
    PeerID, Result,
    config::CryptoProviderRef,
    peer_cert::{PEER_ID_EXT_OID, parse_cert, verify_cert, verify_cert_signature},
};

static PROTOCOL_VERSIONS: &[&SupportedProtocolVersion] = &[&TLS13];
static CIPHER_SUITES: &[CipherSuite] = &[CipherSuite::TLS13_CHACHA20_POLY1305_SHA256];
/// QUIC also needs AES-128-GCM: quinn uses it to protect the initial packets.
/// ChaCha20 stays first so it is still preferred for the session.
#[cfg(feature = "quic")]
static QUIC_CIPHER_SUITES: &[CipherSuite] = &[
    CipherSuite::TLS13_CHACHA20_POLY1305_SHA256,
    CipherSuite::TLS13_AES_128_GCM_SHA256,
];
static KX_GROUP: NamedGroup = NamedGroup::X25519;
static SIGNATURE_SCHEMES: &[SignatureScheme] = &[SignatureScheme::ED25519];

/// PKCS#8 v1 prefix of an Ed25519 private key (RFC 8410); the 32 byte
/// seed follows it.
const ED25519_PKCS8_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

/// Returns a TLS client configuration.
pub fn tls_client_config(
    key_pair: &KeyPair,
    peer_id: Option<PeerID>,
    provider: Option<&CryptoProviderRef>,
) -> Result<ClientConfig> {
    client_config(key_pair, peer_id, tls_provider(provider, CIPHER_SUITES)?)
}

/// Returns a TLS client configuration for QUIC.
#[cfg(feature = "quic")]
pub fn quic_client_config(
    key_pair: &KeyPair,
    peer_id: Option<PeerID>,
    provider: Option<&CryptoProviderRef>,
) -> Result<ClientConfig> {
    client_config(
        key_pair,
        peer_id,
        tls_provider(provider, QUIC_CIPHER_SUITES)?,
    )
}

/// Returns a TLS server configuration.
pub fn tls_server_config(
    key_pair: &KeyPair,
    provider: Option<&CryptoProviderRef>,
) -> Result<ServerConfig> {
    server_config(key_pair, tls_provider(provider, CIPHER_SUITES)?)
}

/// Returns a TLS server configuration for QUIC.
#[cfg(feature = "quic")]
pub fn quic_server_config(
    key_pair: &KeyPair,
    provider: Option<&CryptoProviderRef>,
) -> Result<ServerConfig> {
    server_config(key_pair, tls_provider(provider, QUIC_CIPHER_SUITES)?)
}

fn client_config(
    key_pair: &KeyPair,
    peer_id: Option<PeerID>,
    provider: CryptoProviderRef,
) -> Result<ClientConfig> {
    let (cert, private_key) = generate_cert(key_pair)?;
    let server_verifier = SrvrCertVerifier { peer_id };

    let client_config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(PROTOCOL_VERSIONS)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(server_verifier))
        .with_client_auth_cert(vec![cert], private_key)?;

    Ok(client_config)
}

fn server_config(key_pair: &KeyPair, provider: CryptoProviderRef) -> Result<ServerConfig> {
    let (cert, private_key) = generate_cert(key_pair)?;
    let client_verifier = CliCertVerifier {};
    let server_config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(PROTOCOL_VERSIONS)?
        .with_client_cert_verifier(Arc::new(client_verifier))
        .with_single_cert(vec![cert], private_key)?;

    Ok(server_config)
}

/// The given provider, or the default one, cut down to the given cipher
/// suites and X25519. The suites and the group are looked up by id.
fn tls_provider(
    provider: Option<&CryptoProviderRef>,
    suites: &[CipherSuite],
) -> Result<CryptoProviderRef> {
    let base = match provider {
        Some(p) => p.clone(),
        None => base_provider()?,
    };

    let mut cipher_suites = Vec::with_capacity(suites.len());
    for id in suites {
        let suite = base
            .cipher_suites
            .iter()
            .find(|s| s.suite() == *id)
            .ok_or_else(|| General(format!("crypto provider lacks cipher suite {id:?}")))?;
        cipher_suites.push(*suite);
    }

    let kx_group = base
        .kx_groups
        .iter()
        .find(|g| g.name() == KX_GROUP)
        .ok_or_else(|| General(format!("crypto provider lacks kx group {KX_GROUP:?}")))?;

    Ok(Arc::new(CryptoProvider {
        cipher_suites,
        kx_groups: vec![*kx_group],
        ..(*base).clone()
    }))
}

/// The provider of the `aws-lc-rs` or `ring` feature.
#[cfg(any(feature = "aws-lc-rs", feature = "ring"))]
fn base_provider() -> Result<CryptoProviderRef> {
    Ok(Arc::new(default_provider()))
}

/// No provider feature: the process default (`CryptoProvider::install_default`).
#[cfg(not(any(feature = "aws-lc-rs", feature = "ring")))]
fn base_provider() -> Result<CryptoProviderRef> {
    let provider = CryptoProvider::get_default().ok_or_else(|| {
        General("no rustls crypto provider: set Config::crypto_provider".to_string())
    })?;
    Ok(provider.clone())
}

/// A karyon key pair as an rcgen signing key, so rcgen needs no crypto
/// backend of its own.
struct CertKey {
    key_pair: KeyPair,
    public_key: Vec<u8>,
}

impl PublicKeyData for CertKey {
    fn der_bytes(&self) -> &[u8] {
        &self.public_key
    }

    fn algorithm(&self) -> &'static SignatureAlgorithm {
        &PKCS_ED25519
    }
}

impl SigningKey for CertKey {
    fn sign(&self, msg: &[u8]) -> StdResult<Vec<u8>, RcgenError> {
        Ok(self.key_pair.sign(msg))
    }
}

/// Generates a certificate and returns both the certificate and the private key.
fn generate_cert<'a>(key_pair: &KeyPair) -> Result<(CertificateDer<'a>, PrivateKeyDer<'a>)> {
    let cert_key_pair = KeyPair::generate(&KeyPairType::Ed25519);
    let public_key = cert_key_pair.public().as_bytes().to_vec();
    let mut pkcs8 = ED25519_PKCS8_PREFIX.to_vec();
    pkcs8.extend_from_slice(&cert_key_pair.secret().0);
    let private_key = PrivateKeyDer::Pkcs8(pkcs8.into());
    let cert_key = CertKey {
        key_pair: cert_key_pair,
        public_key,
    };

    // Add a custom extension to the certificate:
    //   - Sign the certificate's public key with the provided key pair's private key
    //   - Append both the computed signature and the key pair's public key to the extension
    let signature = key_pair.sign(&cert_key.subject_public_key_info());
    let ext_content = yasna::encode_der(&(key_pair.public().as_bytes().to_vec(), signature));
    let mut ext = CustomExtension::from_oid_content(PEER_ID_EXT_OID, ext_content);
    // XXX: Non-critical because rustls rejects unknown critical extensions
    // before our custom verifiers can run. The extension is still validated
    // by verify_cert() which requires it to be present and checks the signature.
    ext.set_criticality(false);

    let mut params = CertificateParams::new(vec![])?;
    params.custom_extensions.push(ext);
    params.serial_number = Some(serial_number(&cert_key.public_key));

    let cert = CertificateDer::from(params.self_signed(&cert_key)?);
    Ok((cert, private_key))
}

/// The serial rcgen derives with its crypto feature: the first 20 bytes of
/// sha256(public key), top bit cleared so it stays positive.
fn serial_number(public_key: &[u8]) -> SerialNumber {
    let mut serial = Sha256::digest(public_key)[..20].to_vec();
    serial[0] &= 0x7f;
    SerialNumber::from(serial)
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
    ) -> StdResult<ServerCertVerified, TlsError> {
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
    ) -> StdResult<HandshakeSignatureValid, TlsError> {
        unreachable!("ONLY SUPPORT tls 13 VERSION")
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> StdResult<HandshakeSignatureValid, TlsError> {
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
    ) -> StdResult<ClientCertVerified, TlsError> {
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
    ) -> StdResult<HandshakeSignatureValid, TlsError> {
        unreachable!("ONLY SUPPORT tls 13 VERSION")
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> StdResult<HandshakeSignatureValid, TlsError> {
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
