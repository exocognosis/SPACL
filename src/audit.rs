use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::crypto::{HybridIdentity, PublicIdentity, SignatureBundle, canonical_json, sha256_hex};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AuditEvent {
    pub kind: String,
    pub actor: String,
    pub subject: String,
    pub detail: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AuditBody {
    pub index: u64,
    pub timestamp_unix_ms: i64,
    pub previous_hash: String,
    pub signer_key_id: String,
    pub event: AuditEvent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AuditRecord {
    pub schema: String,
    pub body: AuditBody,
    pub record_hash: String,
    pub signature: SignatureBundle,
}

pub struct AuditLog {
    path: PathBuf,
    last_index: Option<u64>,
    last_hash: String,
}

impl AuditLog {
    pub fn read(path: &Path) -> Result<Vec<AuditRecord>> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        BufReader::new(file)
            .lines()
            .enumerate()
            .map(|(index, line)| {
                serde_json::from_str(&line?)
                    .with_context(|| format!("parse audit line {}", index + 1))
            })
            .collect()
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut log = Self {
            path,
            last_index: None,
            last_hash: "GENESIS".into(),
        };
        if log.path.exists() {
            for line in BufReader::new(File::open(&log.path)?).lines() {
                let record: AuditRecord = serde_json::from_str(&line?)?;
                log.last_index = Some(record.body.index);
                log.last_hash = record.record_hash;
            }
        }
        Ok(log)
    }

    pub fn open_verified(path: impl Into<PathBuf>, identities: &[PublicIdentity]) -> Result<Self> {
        let path = path.into();
        if path.exists() {
            Self::verify(&path, identities)?;
        }
        Self::open(path)
    }

    pub fn append(&mut self, event: AuditEvent, signer: &HybridIdentity) -> Result<AuditRecord> {
        let body = AuditBody {
            index: self.last_index.map_or(0, |index| index + 1),
            timestamp_unix_ms: Utc::now().timestamp_millis(),
            previous_hash: self.last_hash.clone(),
            signer_key_id: signer.public.key_id.clone(),
            event,
        };
        let body_bytes = canonical_json(&body)?;
        let record_hash = sha256_hex(&body_bytes);
        let signature = signer.sign(record_hash.as_bytes())?;
        let record = AuditRecord {
            schema: "spacl.audit.v1".into(),
            body,
            record_hash: record_hash.clone(),
            signature,
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        serde_json::to_writer(&mut file, &record)?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        self.last_index = Some(record.body.index);
        self.last_hash = record_hash;
        Ok(record)
    }

    pub fn verify(path: &Path, identities: &[PublicIdentity]) -> Result<Vec<AuditRecord>> {
        let trusted: HashMap<_, _> = identities
            .iter()
            .map(|identity| (identity.key_id.as_str(), identity))
            .collect();
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let mut records = Vec::new();
        let mut expected_previous = "GENESIS".to_string();
        for (line_number, line) in BufReader::new(file).lines().enumerate() {
            let record: AuditRecord = serde_json::from_str(&line?)
                .with_context(|| format!("parse audit line {}", line_number + 1))?;
            if record.body.index != line_number as u64 {
                bail!("audit index mismatch at line {}", line_number + 1)
            }
            if record.body.previous_hash != expected_previous {
                bail!("audit chain mismatch at line {}", line_number + 1)
            }
            let computed = sha256_hex(&canonical_json(&record.body)?);
            if computed != record.record_hash {
                bail!("audit record hash mismatch at line {}", line_number + 1)
            }
            let identity = trusted
                .get(record.body.signer_key_id.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!("untrusted audit signer at line {}", line_number + 1)
                })?;
            identity
                .verify(record.record_hash.as_bytes(), &record.signature)
                .with_context(|| format!("verify audit signature at line {}", line_number + 1))?;
            expected_previous = record.record_hash.clone();
            records.push(record);
        }
        Ok(records)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
