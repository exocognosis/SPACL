use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{PublicIdentity, SignatureBundle};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RobotAction {
    pub skill: String,
    #[serde(default)]
    pub arguments: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub requested_speed_mps: Option<f64>,
    #[serde(default)]
    pub requested_force_newtons: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExecutionContext {
    pub task_id: String,
    pub zone: String,
    pub state_hash: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PolicyConstraints {
    #[serde(default)]
    pub allowed_skills: Vec<String>,
    #[serde(default)]
    pub allowed_zones: Vec<String>,
    #[serde(default)]
    pub max_speed_mps: Option<f64>,
    #[serde(default)]
    pub max_force_newtons: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Normal,
    High,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Approval {
    pub operator_id: String,
    pub approved_at_unix_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActionTokenClaims {
    pub token_id: Uuid,
    pub issuer_key_id: String,
    pub robot_id: String,
    pub action: RobotAction,
    pub sequence: u64,
    pub context_hash: String,
    pub issued_at_unix_ms: i64,
    pub expires_at_unix_ms: i64,
    pub constraints: PolicyConstraints,
    pub risk: RiskLevel,
    #[serde(default)]
    pub approvals: Vec<Approval>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SignedActionToken {
    pub schema: String,
    pub claims: ActionTokenClaims,
    pub signature: SignatureBundle,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenRequest {
    pub robot_id: String,
    pub action: RobotAction,
    pub context: ExecutionContext,
    #[serde(default = "default_ttl")]
    pub ttl_seconds: u64,
    #[serde(default)]
    pub constraints: PolicyConstraints,
    #[serde(default)]
    pub risk: RiskLevel,
    #[serde(default)]
    pub approvals: Vec<Approval>,
}

fn default_ttl() -> u64 {
    30
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RobotRegistration {
    pub robot_id: String,
    pub display_name: String,
    pub identity: PublicIdentity,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RobotRecord {
    pub robot_id: String,
    pub display_name: String,
    pub identity: PublicIdentity,
    pub revoked: bool,
    pub next_sequence: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub token: SignedActionToken,
    pub context: ExecutionContext,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Completed,
    Rejected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionReceipt {
    pub token_id: Uuid,
    pub robot_id: String,
    pub sequence: u64,
    pub status: ExecutionStatus,
    pub detail: String,
    pub completed_at_unix_ms: i64,
}
