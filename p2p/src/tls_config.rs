use std::sync::Arc;

use async_rustls::rustls::{
    self, cipher_suite::TLS13_CHACHA20_POLY1305_SHA256, client::ServerCertVerifier,
    server::ClientCertVerifier, Certificate, CertificateError, Error::InvalidCertificate,
    PrivateKey, SupportedCipherSuite, SupportedKxGroup, SupportedProtocolVersion,
};
use log::error;
use x509_parser::{certificate::X509Certificate, parse_x509_certificate};

use karyon_core::crypto::{KeyPair, KeyPairType, PublicKey};

use crate::{PeerID, Result};

// NOTE: This code needs a comprehensive audit.

static PROTOCOL_VERSIONS: &[&SupportedProtocolVersion] = &[&rustls::version::TLS13];
static CIPHER_SUITES: &[SupportedCipherSuite] = &[TLS13_CHACHA20_POLY1305_SHA256];
static KX_GROUPS: &[&SupportedKxGroup] = &[&rustls::kx_group::X25519];

const BAD_SIGNATURE_ERR: rustls::Error = InvalidCertificate(CertificateError::BadSignature);
const BAD_ENCODING_ERR: rustls::Error = InvalidCertificate(CertificateError::BadEncoding);

/// Returns a TLS client configuration.
pub fn tls_client_config(
    key_pair: &KeyPair,
    peer_id: Option<PeerID>,
) -> Result<rustls::ClientConfig> {
    let (cert, private_key) = generate_cert(key_pair)?;
    let server_verifier = SrvrCertVerifier { peer_id };
    let client_config = rustls::ClientConfig::builder()
        .with_cipher_suites(CIPHER_SUITES)
        .with_kx_groups(KX_GROUPS)
        .with_protocol_versions(PROTOCOL_VERSIONS)?
        .with_custom_certificate_verifier(Arc::new(server_verifier))
        .with_client_auth_cert(vec![cert], private_key)?;

    Ok(client_config)
}

/// Returns a TLS server configuration.
pub fn tls_server_config(key_pair: &KeyPair) -> Result<rustls::ServerConfig> {
    let (cert, private_key) = generate_cert(key_pair)?;
    let client_verifier = CliCertVerifier {};
    let server_config = rustls::ServerConfig::builder()
        .with_cipher_suites(CIPHER_SUITES)
        .with_kx_groups(KX_GROUPS)
        .with_protocol_versions(PROTOCOL_VERSIONS)?
        .with_client_cert_verifier(Arc::new(client_verifier))
        .with_single_cert(vec![cert], private_key)?;

    Ok(server_config)
}

/// Generates a certificate and returns both the certificate and the private key.
fn generate_cert(key_pair: &KeyPair) -> Result<(Certificate, PrivateKey)> {
    let cert_key_pair = rcgen::KeyPair::generate(&rcgen::PKCS_ED25519)?;
    let private_key = rustls::PrivateKey(cert_key_pair.serialize_der());

    // Add a custom extension to the certificate:
    //   - Sign the certificate's public key with the provided key pair's public key
    //   - Append both the signature and the key pair's public key to the extension
    let signature = key_pair.sign(&cert_key_pair.public_key_der());
    let ext_content = yasna::encode_der(&(key_pair.public().as_bytes().to_vec(), signature));
    // XXX: Not sure about the oid number ???
    let mut ext = rcgen::CustomExtension::from_oid_content(&[0, 0, 0, 0], ext_content);
    ext.set_criticality(true);

    let mut params = rcgen::CertificateParams::new(vec![]);
    params.alg = &rcgen::PKCS_ED25519;
    params.key_pair = Some(cert_key_pair);
    params.custom_extensions.push(ext);

    let cert = rustls::Certificate(rcgen::Certificate::from_params(params)?.serialize_der()?);
    Ok((cert, private_key))
}

/// Verifies the given certification.
fn verify_cert(end_entity: &Certificate) -> std::result::Result<PeerID, rustls::Error> {
    // Parse the certificate.
    let cert = parse_cert(end_entity)?;

    match cert.extensions().first() {
        Some(ext) => {
            // Extract the peer id (public key) and the signature from the extension.
            let (public_key, signature): (Vec<u8>, Vec<u8>) =
                yasna::decode_der(ext.value).map_err(|_| BAD_ENCODING_ERR)?;

            // Use the peer id (public key) to verify the extracted signature.
            let public_key = PublicKey::from_bytes(&KeyPairType::Ed25519, &public_key)
                .map_err(|_| BAD_ENCODING_ERR)?;
            public_key
                .verify(cert.public_key().raw, &signature)
                .map_err(|_| BAD_SIGNATURE_ERR)?;

            // Verify the certificate signature.
            verify_cert_signature(
                &cert,
                cert.tbs_certificate.as_ref(),
                cert.signature_value.as_ref(),
            )?;

            PeerID::try_from(public_key).map_err(|_| BAD_ENCODING_ERR)
        }
        None => Err(BAD_ENCODING_ERR),
    }
}

/// Parses the given x509 certificate.
fn parse_cert(end_entity: &Certificate) -> std::result::Result<X509Certificate, rustls::Error> {
    let (_, cert) = parse_x509_certificate(end_entity.as_ref()).map_err(|_| BAD_ENCODING_ERR)?;

    if !cert.validity().is_valid() {
        return Err(InvalidCertificate(CertificateError::NotValidYet));
    }

    Ok(cert)
}

/// Verifies the signature of the given certificate.
fn verify_cert_signature(
    cert: &X509Certificate,
    message: &[u8],
    signature: &[u8],
) -> std::result::Result<(), rustls::Error> {
    let public_key = PublicKey::from_bytes(
        &KeyPairType::Ed25519,
        cert.tbs_certificate.subject_pki.subject_public_key.as_ref(),
    )
    .map_err(|_| BAD_ENCODING_ERR)?;

    public_key
        .verify(message, signature)
        .map_err(|_| BAD_SIGNATURE_ERR)
}

struct SrvrCertVerifier {
    peer_id: Option<PeerID>,
}

impl ServerCertVerifier for SrvrCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> std::result::Result<rustls::client::ServerCertVerified, rustls::Error> {
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
        if let Some(pid) = &self.peer_id {
            if pid != &peer_id {
                return Err(InvalidCertificate(
                    CertificateError::ApplicationVerificationFailure,
                ));
            }
        }

        Ok(rustls::client::ServerCertVerified::assertion())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &Certificate,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::HandshakeSignatureValid, rustls::Error> {
        let cert = parse_cert(cert)?;
        verify_cert_signature(&cert, message, dss.signature())?;
        Ok(rustls::client::HandshakeSignatureValid::assertion())
    }
}

struct CliCertVerifier {}
impl ClientCertVerifier for CliCertVerifier {
    fn verify_client_cert(
        &self,
        end_entity: &Certificate,
        _intermediates: &[Certificate],
        _now: std::time::SystemTime,
    ) -> std::result::Result<rustls::server::ClientCertVerified, rustls::Error> {
        if let Err(err) = verify_cert(end_entity) {
            error!("Failed to verify cert: {err}");
            return Err(err);
        };
        Ok(rustls::server::ClientCertVerified::assertion())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &Certificate,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::HandshakeSignatureValid, rustls::Error> {
        let cert = parse_cert(cert)?;
        verify_cert_signature(&cert, message, dss.signature())?;
        Ok(rustls::client::HandshakeSignatureValid::assertion())
    }

    fn client_auth_root_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }
}

#[cfg(test)]
mod tests {
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
