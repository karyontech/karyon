use ed25519_dalek::{Signer as _, Verifier as _};
use rand::rngs::OsRng;

use crate::{error::Error, Result};

/// key cryptography type
pub enum KeyPairType {
    Ed25519,
}

/// A Secret key
pub struct SecretKey(pub Vec<u8>);

#[derive(Clone)]
pub enum KeyPair {
    Ed25519(Ed25519KeyPair),
}

impl KeyPair {
    /// Generate a new random keypair.
    pub fn generate(kp_type: &KeyPairType) -> Self {
        match kp_type {
            KeyPairType::Ed25519 => Self::Ed25519(Ed25519KeyPair::generate()),
        }
    }

    /// Sign a message using the private key.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        match self {
            KeyPair::Ed25519(kp) => kp.sign(msg),
        }
    }

    /// Get the public key of this keypair.
    pub fn public(&self) -> PublicKey {
        match self {
            KeyPair::Ed25519(kp) => kp.public(),
        }
    }

    /// Get the secret key of this keypair.
    pub fn secret(&self) -> SecretKey {
        match self {
            KeyPair::Ed25519(kp) => kp.secret(),
        }
    }
}

/// An extension trait, adding essential methods to all [`KeyPair`] types.
trait KeyPairExt {
    /// Sign a message using the private key.
    fn sign(&self, msg: &[u8]) -> Vec<u8>;

    /// Get the public key of this keypair.
    fn public(&self) -> PublicKey;

    /// Get the secret key of this keypair.
    fn secret(&self) -> SecretKey;
}

#[derive(Clone)]
pub struct Ed25519KeyPair(ed25519_dalek::SigningKey);

impl Ed25519KeyPair {
    fn generate() -> Self {
        Self(ed25519_dalek::SigningKey::generate(&mut OsRng))
    }
}

impl KeyPairExt for Ed25519KeyPair {
    fn sign(&self, msg: &[u8]) -> Vec<u8> {
        self.0.sign(msg).to_bytes().to_vec()
    }

    fn public(&self) -> PublicKey {
        PublicKey::Ed25519(Ed25519PublicKey(self.0.verifying_key()))
    }

    fn secret(&self) -> SecretKey {
        SecretKey(self.0.to_bytes().to_vec())
    }
}

#[derive(Debug)]
pub enum PublicKey {
    Ed25519(Ed25519PublicKey),
}

impl PublicKey {
    pub fn from_bytes(kp_type: &KeyPairType, pk: &[u8]) -> Result<Self> {
        match kp_type {
            KeyPairType::Ed25519 => Ok(Self::Ed25519(Ed25519PublicKey::from_bytes(pk)?)),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Ed25519(pk) => pk.as_bytes(),
        }
    }

    /// Verify a signature on a message with this public key.
    pub fn verify(&self, msg: &[u8], signature: &[u8]) -> Result<()> {
        match self {
            Self::Ed25519(pk) => pk.verify(msg, signature),
        }
    }
}

/// An extension trait, adding essential methods to all [`PublicKey`] types.
trait PublicKeyExt {
    fn as_bytes(&self) -> &[u8];

    /// Verify a signature on a message with this public key.
    fn verify(&self, msg: &[u8], signature: &[u8]) -> Result<()>;
}

#[derive(Debug)]
pub struct Ed25519PublicKey(ed25519_dalek::VerifyingKey);

impl Ed25519PublicKey {
    pub fn from_bytes(pk: &[u8]) -> Result<Self> {
        let pk_bytes: [u8; 32] = pk
            .try_into()
            .map_err(|_| Error::TryInto("Failed to convert slice to [u8; 32]"))?;

        Ok(Self(ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes)?))
    }
}

impl PublicKeyExt for Ed25519PublicKey {
    fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    fn verify(&self, msg: &[u8], signature: &[u8]) -> Result<()> {
        let sig_bytes: [u8; 64] = signature
            .try_into()
            .map_err(|_| Error::TryInto("Failed to convert slice to [u8; 64]"))?;
        self.0
            .verify(msg, &ed25519_dalek::Signature::from_bytes(&sig_bytes))?;
        Ok(())
    }
}
