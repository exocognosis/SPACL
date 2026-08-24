# Architecture

## System Boundary

SPACL v0.1.0 has two service roles.

The coordination node enrolls robot identities. It assigns token sequence numbers. It signs action tokens. It also records enrollment, revocation, and token issuance in its audit chain.

Each robot runtime pins the coordination node public identity. The runtime verifies each action token before it calls a skill adapter. The current adapter simulates four skills: `move`, `pick`, `place`, and `wait`.

## Data Flow

1. Generate a robot identity on the robot host.
2. Send only the robot public identity to the coordination node.
3. Submit an action, execution context, policy limits, and approvals to the coordination node.
4. The coordination node assigns the next sequence number.
5. The coordination node signs the complete token claims with ML-DSA-65 and Ed25519.
6. Send the token and current execution context to the target robot runtime.
7. The runtime verifies all gates before it calls the skill adapter.
8. The runtime commits the consumed token and next sequence number to disk.
9. The runtime writes signed start and completion records to its audit chain.

## Trust Boundaries

### Coordination Node

The coordination node can authorize actions for all enrolled robots. A stolen coordination private key permits token forgery. Protect this host and its key file.

### Robot Runtime

The robot runtime controls release to the low-level adapter. An attacker with control of this process can bypass SPACL. A production design must put a separate physical safety controller below SPACL.

### Operator API

The v0.1.0 API has no user authentication. Approval records state operator IDs, but they do not prove that those operators approved the action. Use this interface only in isolated development environments.

### Network

The v0.1.0 HTTP interfaces are plaintext. They do not provide confidentiality, endpoint authentication, or downgrade protection. Use loopback or a separate authenticated tunnel during development.

## State and Recovery

The coordination node writes robot state to an atomic JSON snapshot. It writes audit records to JSON Lines and calls `sync_data` after each record.

Each robot runtime persists its next sequence number, consumed token IDs, and emergency-stop state. The runtime rejects a later token until it consumes earlier sequence numbers. An operator must reconcile an abandoned token before the coordinator issues replacement work. Automated reconciliation is a Phase 1 task.

The current release has one coordination node. It has no consensus or automatic failover.

