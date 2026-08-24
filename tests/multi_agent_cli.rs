use std::{fs, process::Command};

use spacl::{AuditLog, PublicIdentity};

#[test]
fn demo_distributes_tokens_and_shows_shared_task_state() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let output_dir = temporary.path().join("multi-agent");
    let output = Command::new(env!("CARGO_BIN_EXE_spacl"))
        .args(["--no-color", "demo", "--output"])
        .arg(&output_dir)
        .arg("--watch")
        .output()
        .expect("run multi-agent demo");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    for index in 1..=3 {
        assert!(stdout.contains(&format!(
            "task assigned warehouse-task-{index} -> sim-robot-{index}"
        )));
        assert!(stdout.contains(&format!(
            "token distributed coordinator -> sim-robot-{index}"
        )));
        assert!(stdout.contains(&format!("valid token accepted sim-robot-{index}")));
    }
    assert!(stdout.contains("invalid token rejected sim-robot-1"));
    assert!(stdout.contains("shared task state"));
    assert!(stdout.contains("audit chains verified"));
    assert!(stdout.contains("coordinator records=9"));
    assert!(stdout.contains("sim-robot-1 records=3"));

    let state: serde_json::Value = serde_json::from_slice(
        &fs::read(output_dir.join("coordinator/state.json")).expect("read state"),
    )
    .expect("parse state");
    let tasks = state["tasks"].as_object().expect("task map");
    assert_eq!(tasks.len(), 3);
    for index in 1..=3 {
        let task_id = format!("warehouse-task-{index}");
        assert_eq!(tasks[&task_id]["robot_id"], format!("sim-robot-{index}"));
    }

    let coordinator_public: PublicIdentity = serde_json::from_slice(
        &fs::read(output_dir.join("coordinator/coordinator.public.json"))
            .expect("read coordinator public identity"),
    )
    .expect("parse coordinator public identity");
    let coordinator_records = AuditLog::verify(
        &output_dir.join("coordinator/audit.jsonl"),
        &[coordinator_public],
    )
    .expect("verify coordinator audit");
    assert_eq!(coordinator_records.len(), 9);

    for index in 1..=3 {
        let robot_id = format!("sim-robot-{index}");
        assert!(output_dir.join(&robot_id).join("token.json").exists());
        let public_identity: PublicIdentity = serde_json::from_slice(
            &fs::read(output_dir.join(&robot_id).join("public.json"))
                .expect("read robot public identity"),
        )
        .expect("parse robot public identity");
        let records = AuditLog::verify(
            &output_dir.join(&robot_id).join("audit.jsonl"),
            &[public_identity],
        )
        .expect("verify robot audit");
        assert_eq!(records.len(), if index == 1 { 3 } else { 2 });
    }
}
