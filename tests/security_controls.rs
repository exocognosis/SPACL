use std::{collections::BTreeMap, fs};

use chrono::Utc;
use spacl::crypto::canonical_json;
use spacl::{
    Approval, AuditEvent, AuditLog, Coordinator, ExecutionContext, ExecutionError, HybridIdentity,
    PolicyConstraints, RiskLevel, RobotAction, RobotRegistration, RobotRuntime, TokenRequest,
};

struct Fixture {
    _temp: tempfile::TempDir,
    coordinator: Coordinator,
    robot_identity: HybridIdentity,
    context: ExecutionContext,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut coordinator = Coordinator::open(temp.path().join("coordinator")).unwrap();
        let robot_identity = HybridIdentity::generate("robot-1");
        coordinator
            .enroll(RobotRegistration {
                robot_id: "robot-1".into(),
                display_name: "Test Robot".into(),
                identity: robot_identity.public.clone(),
            })
            .unwrap();
        Self {
            _temp: temp,
            coordinator,
            robot_identity,
            context: ExecutionContext {
                task_id: "task-1".into(),
                zone: "safe-zone".into(),
                state_hash: "world-1".into(),
            },
        }
    }

    fn request(&self) -> TokenRequest {
        TokenRequest {
            robot_id: "robot-1".into(),
            action: RobotAction {
                skill: "move".into(),
                arguments: BTreeMap::new(),
                requested_speed_mps: Some(0.5),
                requested_force_newtons: None,
            },
            context: self.context.clone(),
            ttl_seconds: 30,
            constraints: PolicyConstraints {
                allowed_skills: vec!["move".into()],
                allowed_zones: vec!["safe-zone".into()],
                max_speed_mps: Some(1.0),
                max_force_newtons: None,
            },
            risk: RiskLevel::Normal,
            approvals: vec![],
        }
    }

    fn runtime(&self) -> RobotRuntime {
        RobotRuntime::open(
            "robot-1",
            self.robot_identity.clone(),
            self.coordinator.identity.public.clone(),
            self._temp.path().join("runtime"),
        )
        .unwrap()
    }
}

#[test]
fn valid_token_executes_once_and_replay_fails() {
    let mut fixture = Fixture::new();
    let token = fixture.coordinator.issue_token(fixture.request()).unwrap();
    let mut runtime = fixture.runtime();
    let receipt = runtime.execute(&token, &fixture.context).unwrap();
    assert_eq!(receipt.sequence, 0);
    assert!(matches!(
        runtime.execute(&token, &fixture.context),
        Err(ExecutionError::Replay)
    ));
}

#[test]
fn tampering_breaks_the_hybrid_signature() {
    let mut fixture = Fixture::new();
    let mut token = fixture.coordinator.issue_token(fixture.request()).unwrap();
    token.claims.action.skill = "place".into();
    let runtime = fixture.runtime();
    assert!(matches!(
        runtime.verify(&token, &fixture.context),
        Err(ExecutionError::InvalidSignature(_))
    ));
}

#[test]
fn context_and_policy_are_enforced() {
    let mut fixture = Fixture::new();
    let token = fixture.coordinator.issue_token(fixture.request()).unwrap();
    let runtime = fixture.runtime();
    let wrong_context = ExecutionContext {
        zone: "restricted-zone".into(),
        ..fixture.context.clone()
    };
    assert!(matches!(
        runtime.verify(&token, &wrong_context),
        Err(ExecutionError::Context)
    ));
}

#[test]
fn sequence_and_emergency_stop_are_enforced() {
    let mut fixture = Fixture::new();
    let first = fixture.coordinator.issue_token(fixture.request()).unwrap();
    let second = fixture.coordinator.issue_token(fixture.request()).unwrap();
    let mut runtime = fixture.runtime();
    assert!(matches!(
        runtime.verify(&second, &fixture.context),
        Err(ExecutionError::Sequence { .. })
    ));
    runtime.set_emergency_stop(true).unwrap();
    assert!(matches!(
        runtime.verify(&first, &fixture.context),
        Err(ExecutionError::EmergencyStop)
    ));
}

#[test]
fn high_risk_action_requires_two_distinct_operators() {
    let mut fixture = Fixture::new();
    let mut request = fixture.request();
    request.risk = RiskLevel::High;
    request.approvals = vec![Approval {
        operator_id: "alice".into(),
        approved_at_unix_ms: Utc::now().timestamp_millis(),
    }];
    assert!(fixture.coordinator.issue_token(request.clone()).is_err());
    request.approvals.push(Approval {
        operator_id: "bob".into(),
        approved_at_unix_ms: Utc::now().timestamp_millis(),
    });
    assert!(fixture.coordinator.issue_token(request).is_ok());
}

#[test]
fn revoked_robot_cannot_receive_tokens() {
    let mut fixture = Fixture::new();
    fixture.coordinator.revoke("robot-1").unwrap();
    assert!(fixture.coordinator.issue_token(fixture.request()).is_err());
}

#[test]
fn enrollment_binds_subject_and_blocks_replacement() {
    let mut fixture = Fixture::new();
    let wrong_subject = HybridIdentity::generate("another-robot");
    assert!(
        fixture
            .coordinator
            .enroll(RobotRegistration {
                robot_id: "robot-2".into(),
                display_name: "Wrong Subject".into(),
                identity: wrong_subject.public.clone(),
            })
            .is_err()
    );
    let replacement = HybridIdentity::generate("robot-1");
    assert!(
        fixture
            .coordinator
            .enroll(RobotRegistration {
                robot_id: "robot-1".into(),
                display_name: "Replacement".into(),
                identity: replacement.public.clone(),
            })
            .is_err()
    );
}

#[test]
fn issuer_claim_must_match_the_pinned_coordinator() {
    let mut fixture = Fixture::new();
    let mut token = fixture.coordinator.issue_token(fixture.request()).unwrap();
    token.claims.issuer_key_id = HybridIdentity::generate("attacker").public.key_id.clone();
    token.signature = fixture
        .coordinator
        .identity
        .sign(&canonical_json(&token.claims).unwrap())
        .unwrap();
    let runtime = fixture.runtime();
    assert!(matches!(
        runtime.verify(&token, &fixture.context),
        Err(ExecutionError::InvalidSignature(_))
    ));
}

#[test]
fn expired_and_over_limit_tokens_are_rejected() {
    let mut fixture = Fixture::new();
    let mut expired = fixture.coordinator.issue_token(fixture.request()).unwrap();
    expired.claims.expires_at_unix_ms = Utc::now().timestamp_millis() - 1;
    expired.signature = fixture
        .coordinator
        .identity
        .sign(&canonical_json(&expired.claims).unwrap())
        .unwrap();
    let runtime = fixture.runtime();
    assert!(matches!(
        runtime.verify(&expired, &fixture.context),
        Err(ExecutionError::Expired)
    ));

    let mut policy_fixture = Fixture::new();
    let mut request = policy_fixture.request();
    request.action.requested_speed_mps = Some(2.0);
    let over_limit = policy_fixture.coordinator.issue_token(request).unwrap();
    let runtime = policy_fixture.runtime();
    assert!(matches!(
        runtime.verify(&over_limit, &policy_fixture.context),
        Err(ExecutionError::Policy(_))
    ));
}

#[test]
fn audit_chain_detects_tampering() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("audit.jsonl");
    let identity = HybridIdentity::generate("auditor");
    let mut log = AuditLog::open(&path).unwrap();
    log.append(
        AuditEvent {
            kind: "test".into(),
            actor: "tester".into(),
            subject: "item".into(),
            detail: serde_json::json!({"value": 1}),
        },
        &identity,
    )
    .unwrap();
    assert_eq!(
        AuditLog::verify(&path, std::slice::from_ref(&identity.public))
            .unwrap()
            .len(),
        1
    );
    let contents = fs::read_to_string(&path)
        .unwrap()
        .replace("\"value\":1", "\"value\":2");
    fs::write(&path, contents).unwrap();
    assert!(AuditLog::verify(&path, &[identity.public.clone()]).is_err());
}

#[test]
fn status_reports_persisted_last_activity() {
    let mut fixture = Fixture::new();
    fixture.coordinator.issue_token(fixture.request()).unwrap();
    assert_eq!(
        fixture.coordinator.status()["last_activity"]["kind"],
        "token.issued"
    );

    let token = fixture.coordinator.issue_token(fixture.request()).unwrap();
    let mut runtime = fixture.runtime();
    assert!(runtime.execute(&token, &fixture.context).is_err());
    assert_eq!(
        runtime.status()["last_activity"]["kind"],
        "execution.rejected"
    );
}
