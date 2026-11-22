//! The peer id in a karyon certificate: the node's key signs the
//! certificate's key, in a custom extension. Used by the built-in TLS and
//! QUIC, and for the certificates a user transport returns.

#[cfg(feature = "smol")]
use futures_rustls::rustls;

#[cfg(feature = "tokio")]
use tokio_rustls::rustls;

use rustls::{CertificateError, Error::InvalidCertificate};
use rustls_pki_types::CertificateDer;
use x509_parser::{asn1_rs::Oid, certificate::X509Certificate, parse_x509_certificate};

use karyon_core::crypto::{KeyPairType, PublicKey};

use crate::PeerID;

/// OID for the karyon peer-id custom extension.
// TODO: not standards-conformant. Replace with a registered one.
pub(crate) const PEER_ID_EXT_OID: &[u64] = &[0, 0, 0, 0];

const BAD_SIGNATURE_ERR: rustls::Error = InvalidCertificate(CertificateError::BadSignature);
const BAD_ENCODING_ERR: rustls::Error = InvalidCertificate(CertificateError::BadEncoding);

/// Derive the peer id from a peer's certificate chain post-handshake.
/// Reuses `verify_cert` so the validation rules stay in one place.
/// Returns `None` if the chain is empty or the cert lacks our extension.
pub(crate) fn peer_id_from_certs(certs: &[CertificateDer<'_>]) -> Option<PeerID> {
    let end_entity = certs.first()?;
    verify_cert(end_entity).ok()
}

/// Verifies the given certification.
pub(crate) fn verify_cert(
    end_entity: &CertificateDer<'_>,
) -> std::result::Result<PeerID, rustls::Error> {
    // Parse the certificate.
    let cert = parse_cert(end_entity)?;

    // Find our custom extension by OID, not by position.
    let want_oid = Oid::from(PEER_ID_EXT_OID).map_err(|_| BAD_ENCODING_ERR)?;
    let ext = cert
        .extensions()
        .iter()
        .find(|e| e.oid == want_oid)
        .ok_or(BAD_ENCODING_ERR)?;

    // Extract the peer id (public key) and the signature from the extension.
    let (public_key, signature): (Vec<u8>, Vec<u8>) =
        yasna::decode_der(ext.value).map_err(|_| BAD_ENCODING_ERR)?;

    // Use the peer id (public key) to verify the extracted signature.
    let public_key =
        PublicKey::from_bytes(&KeyPairType::Ed25519, &public_key).map_err(|_| BAD_ENCODING_ERR)?;
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

/// Parses the given x509 certificate.
pub(crate) fn parse_cert<'a>(
    end_entity: &'a CertificateDer<'a>,
) -> std::result::Result<X509Certificate<'a>, rustls::Error> {
    let (_, cert) = parse_x509_certificate(end_entity.as_ref()).map_err(|_| BAD_ENCODING_ERR)?;

    if !cert.validity().is_valid() {
        return Err(InvalidCertificate(CertificateError::NotValidYet));
    }

    Ok(cert)
}

/// Verifies the signature of the given certificate.
pub(crate) fn verify_cert_signature(
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
