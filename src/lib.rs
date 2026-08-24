#![doc = r#"
# SPACL

SPACL is a secure coordination and accountability layer for simulated robot fleets.

The library provides hybrid ML-DSA-65 and Ed25519 identities, context-bound action tokens,
robot-side execution gates, persistent coordination state, and signed hash-chain audit records.

Start with [`Coordinator`] to enroll robots and issue tokens. Use [`RobotRuntime`] to verify and
execute those tokens through the Phase 0 simulation adapter.

This release is simulation-only. It does not provide transport authentication, operator
authentication, physical safety certification, or replicated consensus.

See the [project guide](https://github.com/exocognosis/SPACL),
[security model](https://github.com/exocognosis/SPACL/blob/main/docs/security.md), and
[OpenAPI reference](https://github.com/exocognosis/SPACL/blob/main/docs/openapi.yaml).
"#]

pub mod audit;
pub mod coordinator;
pub mod crypto;
pub mod metrics;
pub mod model;
pub mod runtime;

pub use audit::{AuditEvent, AuditLog, AuditRecord};
pub use coordinator::{Coordinator, CoordinatorError};
pub use crypto::{HybridIdentity, PublicIdentity, SignatureBundle};
pub use metrics::Metrics;
pub use model::*;
pub use runtime::{ExecutionError, RobotRuntime};
