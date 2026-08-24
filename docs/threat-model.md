# SPACL Threat Model

## Overview

SPACL is a Rust middleware and runtime for authorized robot actions. The repository contains a coordination node, robot execution gates, a command-line interface, REST development services, persistent JSON state, and signed JSON Lines audit chains. The current controller adapter simulates `move`, `pick`, `place`, and `wait`. It does not control physical hardware.

The coordination node enrolls robot identities, records exclusive task ownership, checks high-risk approval assertions, assigns per-robot sequence numbers, and signs action tokens. Each robot runtime pins the coordinator public identity. It verifies the token before it releases the action to its local adapter.

SPACL v0.2.0 is a simulation MVP for loopback or an isolated test network. It is not a physical safety controller, a production authorization service, or a fault-tolerant coordination cluster.

Primary security assets are:

- Coordinator and robot private identity material.
- Integrity and authenticity of signed action-token claims.
- Correct operation of each robot execution gate.
- Robot enrollment, revocation, task ownership, sequence, replay, and emergency-stop state.
- Integrity, signer identity, order, and availability of audit records.
- Availability of the coordinator and robot runtimes.
- The correctness of the execution context supplied to a robot.

## Threat Model, Trust Boundaries, and Assumptions

### Actors

- **Planner or operator:** Submits action requests, context, limits, and approval assertions. Version 0.2.0 does not authenticate this actor.
- **Coordinator:** Holds the fleet authorization key. It assigns tasks and signs tokens for all enrolled robots.
- **Robot runtime:** Holds one robot identity, pins the coordinator identity, verifies tokens, and controls release to the adapter.
- **Local administrator:** Controls configuration, identity files, bind addresses, process startup, and persistent storage.
- **Network attacker:** Can read, delay, drop, replay, modify, and inject plaintext HTTP traffic when the services are reachable.
- **Compromised host attacker:** Controls a coordinator host or robot host. This attacker can bypass the protections implemented by that host.
- **Developer or dependency publisher:** Can affect builds, dependencies, container images, examples, or release artifacts.

### Trust Boundaries

1. **Planner to coordinator API:** The REST API accepts JSON requests for enrollment, task assignment, token issuance, revocation, and status. The interface uses plaintext HTTP and has no client authentication.
2. **Coordinator to robot runtime:** The transport is untrusted. The runtime relies on the pinned coordinator identity and the hybrid signature over all token claims.
3. **Robot runtime to local adapter:** The runtime is the execution gate. A production controller must not accept a parallel command path that bypasses this gate.
4. **Process to local filesystem:** JSON state, identity files, configuration, tokens, and audit records depend on operating-system file permissions and storage integrity.
5. **Audit record to verifier:** A verifier must use the correct trusted public identity and the complete audit file. The current chain has no external checkpoint that proves the file was not truncated or deleted.
6. **Operator assertion to authorization policy:** High-risk approval entries are signed into a token, but the coordinator does not authenticate or cryptographically verify the named operators.
7. **Runtime to world state:** The context hash protects the supplied task, zone, and state reference from token modification. It does not prove that sensors or the supplied world state are truthful.

### Attacker-Controlled Inputs

- HTTP paths, JSON bodies, robot IDs, public enrollment identities, task IDs, action arguments, constraints, contexts, approval names, tokens, and emergency-stop requests.
- Network timing, message order, duplication, loss, and modification.
- Token and audit files supplied to command-line inspection commands.
- Configuration and identity paths when an attacker already controls the local account or deployment system.

Operator-controlled inputs include bind addresses, data directories, identity files, robot enrollment, revocation, approval assertions, and emergency-stop changes. Developer-controlled inputs include Rust dependencies, the lockfile, CI workflows, Docker build inputs, examples, and the demo-video renderer.

### Security Invariants

SPACL must preserve these invariants:

- A runtime accepts a token only when both the ML-DSA-65 and Ed25519 signatures verify against its pinned coordinator identity.
- The signature covers the robot ID, action, sequence, context hash, validity interval, constraints, risk, and approval assertions.
- A runtime accepts only its own tokens and only the exact current execution context.
- Sequence numbers increase monotonically. A consumed token cannot execute again. A later token cannot skip an earlier sequence.
- Expired or not-yet-active tokens do not execute.
- Skill, zone, speed, and force values remain within the signed policy constraints.
- An active emergency stop prevents token execution.
- A revoked robot cannot receive a new token.
- One task cannot have two robot owners in coordinator state.
- Audit verification fails when a record body, previous-record hash, signer, signature, or index changes.
- Private identity files do not grant group or world access on Unix systems.

### Assumptions and Limits

- The initial coordinator public key pinned by a robot is correct.
- The host operating system, process memory, system clock, and filesystem are trusted.
- The low-level controller cannot bypass the robot runtime.
- Operators use loopback or an isolated test network.
- Private keys remain in software files. No hardware root of trust protects them.
- The coordinator is the single source of task ownership. There is no consensus or automatic failover.
- Physical interlocks and an independent safety controller stop hazardous motion even if SPACL fails.

## Attack Surface, Mitigations, and Attacker Stories

### Coordinator REST API

The coordinator exposes health, metrics, status, fleet, task, enrollment, revocation, and token endpoints in `src/main.rs`. The API has no client authentication, authorization roles, rate limits, or transport encryption. Its Cross-Origin Resource Sharing rule supports a local development console. It is not an authentication control.

A network client that can reach the API can request tokens for enrolled robots, submit approval names, enroll a new identity under an unused robot ID, assign unowned tasks, revoke identities, and read fleet state. This is an accepted development limit, not a safe production configuration. Bind the service to loopback or an isolated network.

Existing controls include typed JSON structures, robot-subject binding at enrollment, identity-key validation, exclusive task ownership, revocation checks, bounded token lifetime, and a two-distinct-name check for high-risk approval assertions.

### Robot REST API and Execution Gate

The robot service exposes status, execution, metrics, and emergency-stop endpoints. A reachable client can set or clear the development emergency-stop flag because the API has no operator authentication. A client can also send arbitrary tokens and contexts to the execution endpoint.

`src/runtime.rs` mitigates token attacks with a pinned issuer key ID, hybrid signature verification, robot targeting, a five-second clock-skew allowance, exact context hashing, sequence checks, replay tracking, an emergency-stop gate, and policy limits. Unknown simulation skills are denied. Rejected, started, and completed executions produce signed local audit events.

The runtime does not protect against an attacker who controls the robot host, replaces the process, changes the pinned key before startup, or sends commands through another controller interface.

### Key and Identity Storage

`src/crypto.rs` stores ML-DSA and Ed25519 private material in JSON. It writes private files with mode `0600` on Unix and rejects files that grant group or world access. Debug output redacts private fields, and the in-memory strings are zeroized when an identity is dropped.

Disk encryption, memory locking, hardware-backed keys, secure boot, process isolation, backup protection, and key rotation are outside the current implementation. Coordinator-key compromise permits token forgery for the full fleet. Robot-key compromise permits forged local audit records for that robot.

### Persistent State and Audit Chains

The coordinator and robot runtimes persist JSON snapshots with temporary-file replacement. Robot state tracks consumed token IDs, sequence, emergency stop, and last activity. Coordinator state tracks enrollment, revocation, sequence, task ownership, and last activity.

`src/audit.rs` verifies each record index, previous-record hash, record hash, trusted signer, and hybrid signature when an audit log opens or an operator verifies it. This detects record modification, insertion, and reordering. It does not prove completeness after deletion or truncation because SPACL does not publish external signed checkpoints or Merkle roots.

A crash between a state write and an audit append can also leave state and audit evidence out of sync. Recovery and reconciliation must fail closed before production use.

### Context, Sensors, and Physical Control

The signed context hash binds a token to the task ID, zone, and state reference supplied at issue time. It blocks later modification of those fields. It does not attest sensor data, map accuracy, obstacle state, localization, or controller firmware.

A malicious planner or compromised sensor pipeline can request a cryptographically valid but physically unsafe action. Signed authorization is not proof of physical safety. A production deployment needs independent motion limits, collision avoidance, emergency circuits, and site-specific hazard controls below SPACL.

### Availability and Supply Chain

Large or frequent API requests, repeated signature verification, storage exhaustion, corrupt state, network partitions, and coordinator failure can deny service. The current single coordinator has no failover. Availability attacks are important for operations but must not cause the runtime to bypass verification.

`Cargo.lock` fixes the dependency graph. CI runs formatting, Clippy, tests, documentation, and the three-robot demo. These controls reduce accidental changes. They do not replace dependency review, reproducible release builds, artifact signing, or an independent cryptographic audit.

### Out-of-Scope Attacker Stories

- A fully compromised coordinator or robot host is outside the protection boundary of that host. The threat model still treats key extraction and gate bypass as critical production risks.
- Malicious low-level firmware and alternate hardware command paths are outside the simulation adapter.
- Traffic confidentiality and authenticated transport are not claimed in v0.2.0.
- Operator approval names are workflow assertions. They are not proof that two humans approved an action.
- Formal safety certification, Byzantine replicas, side-channel resistance, and hardware fault attacks are not current claims.

## Severity Calibration

Severity depends on reachability and deployment. A flaw can have lower impact in the supported simulation-only, loopback configuration and critical impact if an operator connects the same path to physical equipment.

### Critical

- A signature-verification bypass that lets an untrusted client execute arbitrary actions on a physical robot.
- Remote extraction of the coordinator private key or a flaw that permits fleet-wide token forgery.
- A remote execution-gate bypass or unauthenticated emergency-stop reset in a physical deployment where hazardous motion can result.

### High

- Replay, sequence, expiry, target, context, or policy bypass that enables an unauthorized physical action under realistic deployment conditions.
- Revocation or task-ownership bypass that permits a revoked robot to receive work or two robots to act on the same exclusive task.
- Audit-signature forgery that produces credible false evidence of a physical action.
- Robot private-key disclosure that permits forged evidence for one robot.

### Medium

- A reachable denial of service that stops coordination or exhausts storage but does not bypass a safety control.
- Undetected audit truncation, loss, or state-to-audit inconsistency that reduces accountability without enabling action forgery.
- Exposure of non-secret fleet state, task ownership, metrics, or public identities beyond the intended isolated network.

### Low

- Malformed input that produces a clear rejection with no persistent corruption or sensitive output.
- A defect limited to simulation display, documentation, local demo media, or developer tooling with no effect on token verification, key handling, state integrity, or release provenance.
- Minor observability or error-message problems that do not disclose secrets or change authorization behavior.
