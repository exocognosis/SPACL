use std::{fs, process::Command};

use spacl::{AuditLog, ExecutionReceipt, PublicIdentity, SignedActionToken};

#[test]
fn single_agent_cli_issues_verifies_executes_and_audits() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let output_dir = temporary.path().join("single-agent");
    let output = Command::new(env!("CARGO_BIN_EXE_spacl"))
        .args(["--no-color", "single-agent", "--skill", "move", "--output"])
        .arg(&output_dir)
        .output()
        .expect("run single-agent command");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(stdout.contains("1. token issued"));
    assert!(stdout.contains("2. token verified"));
    assert!(stdout.contains("3. action completed move"));
    assert!(stdout.contains("4. audit chains verified coordinator=3 robot=2"));

    let token: SignedActionToken =
        serde_json::from_slice(&fs::read(output_dir.join("token.json")).expect("read token"))
            .expect("parse token");
    assert_eq!(token.claims.robot_id, "single-robot-1");
    assert_eq!(token.claims.action.skill, "move");
    assert_eq!(token.claims.sequence, 0);

    let receipt: ExecutionReceipt =
        serde_json::from_slice(&fs::read(output_dir.join("receipt.json")).expect("read receipt"))
            .expect("parse receipt");
    assert_eq!(receipt.token_id, token.claims.token_id);

    let coordinator_public: PublicIdentity = serde_json::from_slice(
        &fs::read(output_dir.join("coordinator/coordinator.public.json"))
            .expect("read coordinator identity"),
    )
    .expect("parse coordinator identity");
    let robot_public: PublicIdentity = serde_json::from_slice(
        &fs::read(output_dir.join("single-robot-1/public.json")).expect("read robot identity"),
    )
    .expect("parse robot identity");

    let coordinator_records = AuditLog::verify(
        &output_dir.join("coordinator/audit.jsonl"),
        &[coordinator_public],
    )
    .expect("verify coordinator audit");
    let robot_records = AuditLog::verify(
        &output_dir.join("single-robot-1/audit.jsonl"),
        &[robot_public],
    )
    .expect("verify robot audit");
    assert_eq!(coordinator_records.len(), 3);
    assert_eq!(robot_records.len(), 2);
}
