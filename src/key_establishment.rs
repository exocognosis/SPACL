//! ML-KEM primitives selected for future authenticated transport.
//!
//! This module does not create a secure channel. A transport protocol must bind
//! endpoint identities, transcripts, protocol versions, and derived traffic keys.

use std::fmt;

use anyhow::{Result, anyhow};
use ml_kem::{
    EncapsulationKey768, MlKem768,
    kem::{Decapsulate, Encapsulate, Kem, KeyExport, TryKeyInit},
};
use zeroize::Zeroize;

/// The selected FIPS 203 parameter set for SPACL transport key establishment.
pub const ML_KEM_768_ALGORITHM: &str = "ML-KEM-768";

/// A 32-byte shared secret produced by ML-KEM-768.
///
/// The value is redacted from debug output and zeroized when dropped.
pub struct MlKemSharedSecret([u8; 32]);

impl MlKemSharedSecret {
    fn from_slice(value: &[u8]) -> Result<Self> {
        let bytes = value
            .try_into()
            .map_err(|_| anyhow!("invalid ML-KEM-768 shared secret length"))?;
        Ok(Self(bytes))
    }

    /// Borrow the secret bytes for input to a reviewed key derivation function.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for MlKemSharedSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MlKemSharedSecret([REDACTED])")
    }
}

impl Drop for MlKemSharedSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// The ciphertext and sender-side secret from one ML-KEM-768 encapsulation.
pub struct MlKemEncapsulation {
    /// Send this ciphertext to the holder of the matching decapsulation key.
    pub ciphertext: Vec<u8>,
    /// Use this secret only as input to the transport key schedule.
    pub shared_secret: MlKemSharedSecret,
}

/// An in-memory ML-KEM-768 recipient key pair.
///
/// The private decapsulation key is not serializable through this API. The
/// `ml-kem` crate zeroizes its secret key material when its `zeroize` feature is
/// enabled.
pub struct MlKem768KeyPair {
    decapsulation_key: <MlKem768 as Kem>::DecapsulationKey,
    encapsulation_key: <MlKem768 as Kem>::EncapsulationKey,
}

impl MlKem768KeyPair {
    /// Generate a new ML-KEM-768 key pair with the operating system random source.
    #[must_use]
    pub fn generate() -> Self {
        let (decapsulation_key, encapsulation_key) = MlKem768::generate_keypair();
        Self {
            decapsulation_key,
            encapsulation_key,
        }
    }

    /// Export the public encapsulation key.
    #[must_use]
    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.encapsulation_key.to_bytes().to_vec()
    }

    /// Decapsulate one ML-KEM-768 ciphertext.
    ///
    /// This function rejects an incorrect ciphertext length. FIPS 203 uses
    /// implicit rejection for a correctly sized invalid ciphertext and returns
    /// a pseudorandom shared secret.
    pub fn decapsulate(&self, ciphertext: &[u8]) -> Result<MlKemSharedSecret> {
        let shared_secret = self
            .decapsulation_key
            .decapsulate_slice(ciphertext)
            .map_err(|_| anyhow!("invalid ML-KEM-768 ciphertext length"))?;
        MlKemSharedSecret::from_slice(shared_secret.as_slice())
    }
}

impl fmt::Debug for MlKem768KeyPair {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MlKem768KeyPair")
            .field("algorithm", &ML_KEM_768_ALGORITHM)
            .field("private_material", &"[REDACTED]")
            .finish()
    }
}

/// Encapsulate a fresh shared secret to an ML-KEM-768 public key.
pub fn encapsulate_ml_kem_768(public_key: &[u8]) -> Result<MlKemEncapsulation> {
    let encapsulation_key = EncapsulationKey768::new_from_slice(public_key)
        .map_err(|_| anyhow!("invalid ML-KEM-768 public key"))?;
    let (ciphertext, shared_secret) = encapsulation_key.encapsulate();

    Ok(MlKemEncapsulation {
        ciphertext: ciphertext.to_vec(),
        shared_secret: MlKemSharedSecret::from_slice(shared_secret.as_slice())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ml_kem_768_establishes_the_same_shared_secret() {
        let recipient = MlKem768KeyPair::generate();
        let public_key = recipient.public_key_bytes();
        let sender = encapsulate_ml_kem_768(&public_key).expect("encapsulate");
        let receiver = recipient
            .decapsulate(&sender.ciphertext)
            .expect("decapsulate");

        assert_eq!(public_key.len(), 1_184);
        assert_eq!(sender.ciphertext.len(), 1_088);
        assert_eq!(sender.shared_secret.as_bytes(), receiver.as_bytes());
        assert_eq!(
            format!("{:?}", sender.shared_secret),
            "MlKemSharedSecret([REDACTED])"
        );
    }

    #[test]
    fn ml_kem_768_rejects_incorrect_lengths() {
        let recipient = MlKem768KeyPair::generate();

        assert!(encapsulate_ml_kem_768(&[0_u8; 32]).is_err());
        assert!(recipient.decapsulate(&[0_u8; 32]).is_err());
    }
}
