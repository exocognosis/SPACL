use std::{fmt, fs, path::Path};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer as _, Verifier as _};
use ml_dsa::{
    Generate as _, Keypair as _, MlDsa65, SignatureEncoding as _, SigningKey, VerifyingKey,
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

const DOMAIN: &[u8] = b"SPACL-HYBRID-SIGNATURE-V1\0";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicIdentity {
    pub key_id: String,
    pub subject: String,
    pub algorithm: String,
    pub ml_dsa_65_public_key: String,
    pub ed25519_public_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignatureBundle {
    pub algorithm: String,
    pub key_id: String,
    pub ml_dsa_65: String,
    pub ed25519: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HybridIdentity {
    pub public: PublicIdentity,
    ml_dsa_seed: String,
    ed25519_secret: String,
}

impl HybridIdentity {
    pub fn generate(subject: impl Into<String>) -> Self {
        let subject = subject.into();
        let ml_key = SigningKey::<MlDsa65>::generate();
        let mut ed_secret = [0_u8; 32];
        OsRng.fill_bytes(&mut ed_secret);
        let ed_key = ed25519_dalek::SigningKey::from_bytes(&ed_secret);
        let ml_public = ml_key.verifying_key().encode();
        let ed_public = ed_key.verifying_key().to_bytes();

        let mut id_hash = Sha256::new();
        id_hash.update(ml_public.as_slice());
        id_hash.update(ed_public);
        let key_id = format!("sha256:{}", hex::encode(id_hash.finalize()));

        Self {
            public: PublicIdentity {
                key_id,
                subject,
                algorithm: "ML-DSA-65+Ed25519".into(),
                ml_dsa_65_public_key: URL_SAFE_NO_PAD.encode(ml_public.as_slice()),
                ed25519_public_key: URL_SAFE_NO_PAD.encode(ed_public),
            },
            ml_dsa_seed: URL_SAFE_NO_PAD.encode(ml_key.to_seed().as_slice()),
            ed25519_secret: URL_SAFE_NO_PAD.encode(ed_secret),
        }
    }

    pub fn sign(&self, payload: &[u8]) -> Result<SignatureBundle> {
        let message = domain_message(payload);
        let ml_seed = decode_fixed::<32>(&self.ml_dsa_seed, "ML-DSA seed")?;
        let ml_seed = ml_dsa::Seed::try_from(ml_seed.as_slice())
            .map_err(|_| anyhow::anyhow!("invalid ML-DSA seed length"))?;
        let ml_key = SigningKey::<MlDsa65>::from_seed(&ml_seed);
        let ml_signature: ml_dsa::Signature<MlDsa65> = ml_dsa::Signer::sign(&ml_key, &message);

        let ed_secret = decode_fixed::<32>(&self.ed25519_secret, "Ed25519 secret")?;
        let ed_key = ed25519_dalek::SigningKey::from_bytes(&ed_secret);
        let ed_signature = ed_key.sign(&message);

        Ok(SignatureBundle {
            algorithm: self.public.algorithm.clone(),
            key_id: self.public.key_id.clone(),
            ml_dsa_65: URL_SAFE_NO_PAD.encode(ml_signature.to_bytes().as_slice()),
            ed25519: URL_SAFE_NO_PAD.encode(ed_signature.to_bytes()),
        })
    }

    pub fn save_private(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn load_private(path: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(path)
                .with_context(|| format!("inspect {}", path.display()))?
                .permissions()
                .mode();
            if mode & 0o077 != 0 {
                bail!(
                    "private identity file {} must not grant group or world permissions",
                    path.display()
                )
            }
        }
        let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let identity: Self = serde_json::from_slice(&bytes)?;
        identity.validate_private_material()?;
        Ok(identity)
    }

    pub fn save_public(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(&self.public)?)?;
        Ok(())
    }

    fn validate_private_material(&self) -> Result<()> {
        let probe = b"spacl-key-validation";
        let signature = self.sign(probe)?;
        self.public.verify(probe, &signature)
    }
}

impl fmt::Debug for HybridIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HybridIdentity")
            .field("public", &self.public)
            .field("private_material", &"[REDACTED]")
            .finish()
    }
}

impl Drop for HybridIdentity {
    fn drop(&mut self) {
        self.ml_dsa_seed.zeroize();
        self.ed25519_secret.zeroize();
    }
}

impl PublicIdentity {
    pub fn validate(&self) -> Result<()> {
        if self.subject.trim().is_empty() {
            bail!("identity subject is empty")
        }
        if self.algorithm != "ML-DSA-65+Ed25519" {
            bail!("unsupported identity algorithm")
        }
        let ml_public = URL_SAFE_NO_PAD.decode(&self.ml_dsa_65_public_key)?;
        ml_dsa::EncodedVerifyingKey::<MlDsa65>::try_from(ml_public.as_slice())
            .map_err(|_| anyhow::anyhow!("invalid ML-DSA public key length"))?;
        let ed_public = decode_fixed::<32>(&self.ed25519_public_key, "Ed25519 public key")?;
        ed25519_dalek::VerifyingKey::from_bytes(&ed_public)?;

        let mut id_hash = Sha256::new();
        id_hash.update(&ml_public);
        id_hash.update(ed_public);
        let expected = format!("sha256:{}", hex::encode(id_hash.finalize()));
        if self.key_id != expected {
            bail!("identity key ID does not match the public keys")
        }
        Ok(())
    }

    pub fn verify(&self, payload: &[u8], signature: &SignatureBundle) -> Result<()> {
        self.validate()?;
        if signature.algorithm != "ML-DSA-65+Ed25519" || self.algorithm != signature.algorithm {
            bail!("unsupported hybrid signature algorithm")
        }
        if signature.key_id != self.key_id {
            bail!("signature key ID does not match the trusted identity")
        }

        let message = domain_message(payload);
        let ml_public_bytes = URL_SAFE_NO_PAD.decode(&self.ml_dsa_65_public_key)?;
        let encoded = ml_dsa::EncodedVerifyingKey::<MlDsa65>::try_from(ml_public_bytes.as_slice())
            .map_err(|_| anyhow::anyhow!("invalid ML-DSA public key length"))?;
        let ml_public = VerifyingKey::<MlDsa65>::decode(&encoded);
        let ml_sig_bytes = URL_SAFE_NO_PAD.decode(&signature.ml_dsa_65)?;
        let ml_signature = ml_dsa::Signature::<MlDsa65>::try_from(ml_sig_bytes.as_slice())
            .map_err(|_| anyhow::anyhow!("invalid ML-DSA signature"))?;
        ml_dsa::Verifier::verify(&ml_public, &message, &ml_signature)
            .map_err(|_| anyhow::anyhow!("ML-DSA signature verification failed"))?;

        let ed_public_bytes = decode_fixed::<32>(&self.ed25519_public_key, "Ed25519 public key")?;
        let ed_public = ed25519_dalek::VerifyingKey::from_bytes(&ed_public_bytes)?;
        let ed_sig_bytes = URL_SAFE_NO_PAD.decode(&signature.ed25519)?;
        let ed_signature = ed25519_dalek::Signature::from_slice(&ed_sig_bytes)?;
        ed_public
            .verify(&message, &ed_signature)
            .map_err(|_| anyhow::anyhow!("Ed25519 signature verification failed"))?;
        Ok(())
    }
}

fn domain_message(payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(DOMAIN.len() + payload.len());
    message.extend_from_slice(DOMAIN);
    message.extend_from_slice(payload);
    message
}

fn decode_fixed<const N: usize>(value: &str, label: &str) -> Result<[u8; N]> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .with_context(|| format!("decode {label}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid {label} length"))
}

pub fn sha256_hex(data: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(data)))
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(value)?)
}
