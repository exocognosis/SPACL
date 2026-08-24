use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    AuditEvent, AuditLog, HybridIdentity,
    crypto::{canonical_json, sha256_hex},
    model::{
        ActionTokenClaims, ActivitySummary, RiskLevel, RobotRecord, RobotRegistration,
        SignedActionToken, TokenRequest,
    },
};

#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error("robot is not enrolled: {0}")]
    UnknownRobot(String),
    #[error("robot identity is revoked: {0}")]
    RevokedRobot(String),
    #[error("robot is already enrolled: {0}")]
    AlreadyEnrolled(String),
    #[error("robot identity subject does not match the robot ID")]
    SubjectMismatch,
    #[error("high-risk actions require two distinct operator approvals")]
    TwoPersonApprovalRequired,
    #[error("token TTL must be between 1 and 300 seconds")]
    InvalidTtl,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistentState {
    robots: BTreeMap<String, RobotRecord>,
    #[serde(default)]
    last_activity: Option<ActivitySummary>,
}

pub struct Coordinator {
    pub identity: HybridIdentity,
    state: PersistentState,
    state_path: PathBuf,
    audit: AuditLog,
}

impl Coordinator {
    pub fn open(data_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&data_dir)?;
        let private_path = data_dir.join("coordinator.identity.json");
        let public_path = data_dir.join("coordinator.public.json");
        let identity = if private_path.exists() {
            let identity = HybridIdentity::load_private(&private_path)?;
            if !public_path.exists() {
                identity.save_public(&public_path)?;
            }
            identity
        } else {
            let identity = HybridIdentity::generate("coordinator");
            identity.save_private(&private_path)?;
            identity.save_public(&public_path)?;
            identity
        };
        let state_path = data_dir.join("state.json");
        let state = if state_path.exists() {
            serde_json::from_slice(&fs::read(&state_path)?)?
        } else {
            PersistentState::default()
        };
        let audit = AuditLog::open_verified(
            data_dir.join("audit.jsonl"),
            std::slice::from_ref(&identity.public),
        )?;
        Ok(Self {
            identity,
            state,
            state_path,
            audit,
        })
    }

    pub fn enroll(
        &mut self,
        registration: RobotRegistration,
    ) -> Result<RobotRecord, CoordinatorError> {
        registration.identity.validate()?;
        if registration.identity.subject != registration.robot_id {
            return Err(CoordinatorError::SubjectMismatch);
        }
        if self.state.robots.contains_key(&registration.robot_id) {
            return Err(CoordinatorError::AlreadyEnrolled(registration.robot_id));
        }
        let record = RobotRecord {
            robot_id: registration.robot_id.clone(),
            display_name: registration.display_name,
            identity: registration.identity,
            revoked: false,
            next_sequence: 0,
        };
        self.state
            .robots
            .insert(registration.robot_id.clone(), record.clone());
        self.set_activity("robot.enrolled", &registration.robot_id);
        self.persist()?;
        self.audit.append(
            AuditEvent {
                kind: "robot.enrolled".into(),
                actor: "coordinator".into(),
                subject: registration.robot_id,
                detail: serde_json::json!({"key_id": record.identity.key_id}),
            },
            &self.identity,
        )?;
        Ok(record)
    }

    pub fn revoke(&mut self, robot_id: &str) -> Result<(), CoordinatorError> {
        let record = self
            .state
            .robots
            .get_mut(robot_id)
            .ok_or_else(|| CoordinatorError::UnknownRobot(robot_id.into()))?;
        record.revoked = true;
        self.set_activity("robot.revoked", robot_id);
        self.persist()?;
        self.audit.append(
            AuditEvent {
                kind: "robot.revoked".into(),
                actor: "operator".into(),
                subject: robot_id.into(),
                detail: serde_json::json!({}),
            },
            &self.identity,
        )?;
        Ok(())
    }

    pub fn issue_token(
        &mut self,
        request: TokenRequest,
    ) -> Result<SignedActionToken, CoordinatorError> {
        if !(1..=300).contains(&request.ttl_seconds) {
            return Err(CoordinatorError::InvalidTtl);
        }
        if request.risk == RiskLevel::High {
            let distinct: BTreeSet<_> = request
                .approvals
                .iter()
                .map(|a| a.operator_id.as_str())
                .collect();
            if distinct.len() < 2 {
                return Err(CoordinatorError::TwoPersonApprovalRequired);
            }
        }
        let record = self
            .state
            .robots
            .get_mut(&request.robot_id)
            .ok_or_else(|| CoordinatorError::UnknownRobot(request.robot_id.clone()))?;
        if record.revoked {
            return Err(CoordinatorError::RevokedRobot(request.robot_id));
        }
        let now = Utc::now().timestamp_millis();
        let claims = ActionTokenClaims {
            token_id: Uuid::new_v4(),
            issuer_key_id: self.identity.public.key_id.clone(),
            robot_id: request.robot_id,
            action: request.action,
            sequence: record.next_sequence,
            context_hash: sha256_hex(&canonical_json(&request.context)?),
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + (request.ttl_seconds as i64 * 1_000),
            constraints: request.constraints,
            risk: request.risk,
            approvals: request.approvals,
        };
        let signature = self.identity.sign(&canonical_json(&claims)?)?;
        let token = SignedActionToken {
            schema: "spacl.action-token.v1".into(),
            claims,
            signature,
        };
        record.next_sequence += 1;
        self.set_activity("token.issued", &token.claims.token_id.to_string());
        self.persist()?;
        self.audit.append(AuditEvent {
            kind: "token.issued".into(), actor: "coordinator".into(), subject: token.claims.token_id.to_string(),
            detail: serde_json::json!({"robot_id": token.claims.robot_id, "sequence": token.claims.sequence, "risk": token.claims.risk}),
        }, &self.identity)?;
        Ok(token)
    }

    pub fn robots(&self) -> Vec<RobotRecord> {
        self.state.robots.values().cloned().collect()
    }

    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "role": "coordinator",
            "identity_key_id": self.identity.public.key_id,
            "robot_count": self.state.robots.len(),
            "robots": self.robots(),
            "last_activity": self.state.last_activity,
        })
    }

    fn set_activity(&mut self, kind: &str, subject: &str) {
        self.state.last_activity = Some(ActivitySummary {
            kind: kind.into(),
            subject: subject.into(),
            at_unix_ms: Utc::now().timestamp_millis(),
        });
    }

    fn persist(&self) -> Result<()> {
        let temporary = self.state_path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&self.state)?)?;
        fs::rename(temporary, &self.state_path)?;
        Ok(())
    }
}
