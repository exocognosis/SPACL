use std::{collections::BTreeSet, fs, path::PathBuf};

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AuditEvent, AuditLog, HybridIdentity, PublicIdentity,
    crypto::{canonical_json, sha256_hex},
    model::{ExecutionContext, ExecutionReceipt, ExecutionStatus, RiskLevel, SignedActionToken},
};

const CLOCK_SKEW_MS: i64 = 5_000;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("emergency stop is active")]
    EmergencyStop,
    #[error("unsupported token schema")]
    Schema,
    #[error("token targets another robot")]
    WrongRobot,
    #[error("token signature is invalid: {0}")]
    InvalidSignature(String),
    #[error("token has expired or is not active")]
    Expired,
    #[error("expected sequence {expected}, received {actual}")]
    Sequence { expected: u64, actual: u64 },
    #[error("execution context does not match the token")]
    Context,
    #[error("policy denied action: {0}")]
    Policy(String),
    #[error("token was already executed")]
    Replay,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct RuntimeState {
    next_sequence: u64,
    emergency_stop: bool,
    consumed_tokens: BTreeSet<String>,
}

pub struct RobotRuntime {
    robot_id: String,
    identity: HybridIdentity,
    coordinator: PublicIdentity,
    state: RuntimeState,
    state_path: PathBuf,
    audit: AuditLog,
}

impl RobotRuntime {
    pub fn open(
        robot_id: impl Into<String>,
        identity: HybridIdentity,
        coordinator: PublicIdentity,
        data_dir: PathBuf,
    ) -> Result<Self> {
        let robot_id = robot_id.into();
        if identity.public.subject != robot_id {
            anyhow::bail!("robot identity subject does not match the robot ID")
        }
        coordinator.validate()?;
        fs::create_dir_all(&data_dir)?;
        let state_path = data_dir.join("runtime-state.json");
        let state = if state_path.exists() {
            serde_json::from_slice(&fs::read(&state_path)?)?
        } else {
            RuntimeState::default()
        };
        let audit = AuditLog::open_verified(
            data_dir.join("audit.jsonl"),
            std::slice::from_ref(&identity.public),
        )?;
        Ok(Self {
            robot_id,
            identity,
            coordinator,
            state,
            state_path,
            audit,
        })
    }

    pub fn execute(
        &mut self,
        token: &SignedActionToken,
        context: &ExecutionContext,
    ) -> Result<ExecutionReceipt, ExecutionError> {
        let result = self.verify(token, context);
        if let Err(error) = result {
            let _ = self.audit.append(
                AuditEvent {
                    kind: "execution.rejected".into(),
                    actor: self.robot_id.clone(),
                    subject: token.claims.token_id.to_string(),
                    detail: serde_json::json!({"reason": error.to_string()}),
                },
                &self.identity,
            );
            return Err(error);
        }

        self.audit.append(AuditEvent {
            kind: "execution.started".into(), actor: self.robot_id.clone(), subject: token.claims.token_id.to_string(),
            detail: serde_json::json!({"skill": token.claims.action.skill, "sequence": token.claims.sequence}),
        }, &self.identity)?;

        // The MVP simulator accepts only named skills. A ROS 2 adapter must replace this block
        // before use with physical hardware.
        let detail = match token.claims.action.skill.as_str() {
            "move" | "pick" | "place" | "wait" => {
                format!("simulated {} completed", token.claims.action.skill)
            }
            other => return Err(ExecutionError::Policy(format!("unknown skill {other}"))),
        };

        self.state
            .consumed_tokens
            .insert(token.claims.token_id.to_string());
        self.state.next_sequence += 1;
        self.persist()?;
        let receipt = ExecutionReceipt {
            token_id: token.claims.token_id,
            robot_id: self.robot_id.clone(),
            sequence: token.claims.sequence,
            status: ExecutionStatus::Completed,
            detail,
            completed_at_unix_ms: Utc::now().timestamp_millis(),
        };
        self.audit.append(
            AuditEvent {
                kind: "execution.completed".into(),
                actor: self.robot_id.clone(),
                subject: token.claims.token_id.to_string(),
                detail: serde_json::to_value(&receipt).map_err(anyhow::Error::from)?,
            },
            &self.identity,
        )?;
        Ok(receipt)
    }

    pub fn verify(
        &self,
        token: &SignedActionToken,
        context: &ExecutionContext,
    ) -> Result<(), ExecutionError> {
        if self.state.emergency_stop {
            return Err(ExecutionError::EmergencyStop);
        }
        if token.schema != "spacl.action-token.v1" {
            return Err(ExecutionError::Schema);
        }
        if token.claims.robot_id != self.robot_id {
            return Err(ExecutionError::WrongRobot);
        }
        if token.claims.issuer_key_id != self.coordinator.key_id {
            return Err(ExecutionError::InvalidSignature(
                "token issuer is not the pinned coordinator".into(),
            ));
        }
        self.coordinator
            .verify(&canonical_json(&token.claims)?, &token.signature)
            .map_err(|error| ExecutionError::InvalidSignature(error.to_string()))?;
        let now = Utc::now().timestamp_millis();
        if token.claims.issued_at_unix_ms > now + CLOCK_SKEW_MS
            || token.claims.expires_at_unix_ms < now
        {
            return Err(ExecutionError::Expired);
        }
        if self
            .state
            .consumed_tokens
            .contains(&token.claims.token_id.to_string())
        {
            return Err(ExecutionError::Replay);
        }
        if token.claims.sequence != self.state.next_sequence {
            return Err(ExecutionError::Sequence {
                expected: self.state.next_sequence,
                actual: token.claims.sequence,
            });
        }
        if token.claims.context_hash != sha256_hex(&canonical_json(context)?) {
            return Err(ExecutionError::Context);
        }
        self.check_policy(token, context)
    }

    fn check_policy(
        &self,
        token: &SignedActionToken,
        context: &ExecutionContext,
    ) -> Result<(), ExecutionError> {
        let policy = &token.claims.constraints;
        if !policy.allowed_skills.is_empty()
            && !policy.allowed_skills.contains(&token.claims.action.skill)
        {
            return Err(ExecutionError::Policy("skill is not allowed".into()));
        }
        if !policy.allowed_zones.is_empty() && !policy.allowed_zones.contains(&context.zone) {
            return Err(ExecutionError::Policy("zone is not allowed".into()));
        }
        if let (Some(requested), Some(maximum)) = (
            token.claims.action.requested_speed_mps,
            policy.max_speed_mps,
        ) {
            if !requested.is_finite() || requested < 0.0 || requested > maximum {
                return Err(ExecutionError::Policy(
                    "speed exceeds the token limit".into(),
                ));
            }
        }
        if let (Some(requested), Some(maximum)) = (
            token.claims.action.requested_force_newtons,
            policy.max_force_newtons,
        ) {
            if !requested.is_finite() || requested < 0.0 || requested > maximum {
                return Err(ExecutionError::Policy(
                    "force exceeds the token limit".into(),
                ));
            }
        }
        if token.claims.risk == RiskLevel::High {
            let operators: BTreeSet<_> = token
                .claims
                .approvals
                .iter()
                .map(|approval| &approval.operator_id)
                .collect();
            if operators.len() < 2 {
                return Err(ExecutionError::Policy(
                    "two distinct approvals are required".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn set_emergency_stop(&mut self, active: bool) -> Result<()> {
        self.state.emergency_stop = active;
        self.persist()?;
        self.audit.append(
            AuditEvent {
                kind: if active {
                    "emergency_stop.activated"
                } else {
                    "emergency_stop.cleared"
                }
                .into(),
                actor: self.robot_id.clone(),
                subject: self.robot_id.clone(),
                detail: serde_json::json!({}),
            },
            &self.identity,
        )?;
        Ok(())
    }

    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "robot_id": self.robot_id,
            "next_sequence": self.state.next_sequence,
            "emergency_stop": self.state.emergency_stop,
            "identity_key_id": self.identity.public.key_id,
        })
    }

    fn persist(&self) -> Result<()> {
        let temporary = self.state_path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&self.state)?)?;
        fs::rename(temporary, &self.state_path)?;
        Ok(())
    }
}
