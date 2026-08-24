# Architecture

## System Boundary

SPACL v0.2.0 has two service roles.

The coordination node enrolls robot identities. It stores one robot owner for each task. It rejects a token request when another robot owns the task. It assigns token sequence numbers and signs action tokens. It records enrollment, task assignment, revocation, and token issuance in its audit chain.

Each robot runtime pins the coordination node public identity. The runtime verifies each action token before it calls a skill adapter. The current adapter simulates four skills: `move`, `pick`, `place`, and `wait`.

## Data Flow

1. Generate a robot identity on the robot host.
2. Send only the robot public identity to the coordination node.
3. Submit an action, execution context, policy limits, and approvals to the coordination node.
4. The coordination node assigns an unowned task to the target robot.
5. The coordination node rejects the request if another robot owns the task.
6. The coordination node assigns the next sequence number.
7. The coordination node signs the complete token claims with ML-DSA-65 and Ed25519.
8. Send the token and current execution context to the target robot runtime.
9. The runtime verifies all gates before it calls the skill adapter.
10. The runtime commits the consumed token and next sequence number to disk.
11. The runtime writes signed start and completion records to its audit chain.

## Trust Boundaries

### Coordination Node

The coordination node can authorize actions for all enrolled robots. A stolen coordination private key permits token forgery. Protect this host and its key file.

### Robot Runtime

The robot runtime controls release to the low-level adapter. An attacker with control of this process can bypass SPACL. A production design must put a separate physical safety controller below SPACL.

### Operator API

The v0.2.0 API has no user authentication. Approval records state operator IDs, but they do not prove that those operators approved the action. Use this interface only in isolated development environments.

### Network

The v0.2.0 HTTP interfaces are plaintext. They do not provide confidentiality, endpoint authentication, or downgrade protection. Use loopback or a separate authenticated tunnel during development.

The services allow browser requests only from `http://127.0.0.1:8000` and `http://localhost:8000`. This development Cross-Origin Resource Sharing (CORS) rule supports the static example console. It does not provide authentication.

## State and Recovery

The coordination node writes robot and task ownership state to an atomic JSON snapshot. It writes audit records to JSON Lines and calls `sync_data` after each record.

Task assignment is persistent and exclusive. Version 0.2.0 does not provide task release, reassignment, leases, or replicated state. The coordination node remains the single source of task ownership.

Each robot runtime persists its next sequence number, consumed token IDs, and emergency-stop state. The runtime rejects a later token until it consumes earlier sequence numbers. An operator must reconcile an abandoned token before the coordinator issues replacement work. Automated reconciliation is a Phase 1 task.

The current release has one coordination node. It has no consensus or automatic failover.
