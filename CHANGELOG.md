# Changelog

## Unreleased

- Select and lock RustCrypto `ml-kem` 0.3.2 with ML-KEM-768.
- Add a zeroizing ML-KEM-768 encapsulation and decapsulation primitive with unit tests.
- Document the minimal stack and the boundary between primitive selection and transport security.
- Add a one-command single-agent token, verification, execution, and audit loop.
- Add persistent exclusive task ownership and conflict rejection during token issuance.
- Extend the three-robot demo with token distribution, one invalid-token rejection, shared task state, and verified audit logs.
- Add task ownership API endpoints and status output.
- Make the three-robot demo verify all four audit chains and save each distributed token.
- Prevent interactive token expiry and ensure the invalid-token test always changes the action.
- Add a reproducible 70-second demo video and a README walkthrough.
- Move the operational value proposition to the top of the README and add a concrete warehouse demo scenario.
- Add a repository-scoped threat model with trust boundaries, attacker stories, security invariants, and severity guidance.
- Embed the full demo loop in the top-level README with an animated GIF and a direct raw MP4 link.
- Rewrite the README as one reader path: product, demo, problem, solution, quick start, architecture, features, threat model, specifications, repository layout, documentation, roadmap, and license.
- Move implementation details from the README into focused architecture, security, stack, API, deployment, configuration, ROS 2, and troubleshooting documents.

## 0.2.0

- Add workspace initialization and secure default directories.
- Add TOML configuration for coordinator and robot services.
- Add live and local status reporting with persisted last activity.
- Add CLI wrappers for token issuance and robot execution.
- Add human-readable audit timeline and follow commands.
- Add interactive action selection and watch output to the simulation demo.
- Add machine-readable API errors with stable codes and next actions.
- Add Prometheus request, token, execution, latency, and rejection metrics.
- Add OpenAPI 3.1, a Redoc console, and client examples.
- Add a Justfile, Rustdoc Pages workflow, and CI demo audit artifacts.

## 0.1.0

- Add hybrid ML-DSA-65 and Ed25519 identities and signatures.
- Add context-bound action tokens and robot-side execution gates.
- Add persistent enrollment, revocation, emergency-stop, and sequence state.
- Add signed hash-chain audit logs and a three-robot simulation.
