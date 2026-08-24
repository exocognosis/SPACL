# Security Model and Token Format

## Security Properties

SPACL v0.2.0 implements these properties:

- A robot accepts tokens only from its pinned coordination identity.
- A token signature covers all token claims.
- Both the ML-DSA-65 and Ed25519 signatures must verify.
- A token targets one robot and one execution context.
- A token has one sequence number and a short validity period.
- The runtime rejects consumed, skipped, expired, future, and modified tokens.
- The runtime checks skill, zone, speed, force, risk, and emergency-stop policy.
- Audit record hashes cover the previous record hash and the full event body.
- Every audit record has a hybrid signature from its event source.

## Token Schema

The `spacl.action-token.v1` object contains these signed claims:

| Field | Purpose |
| --- | --- |
| `token_id` | Unique replay identifier |
| `issuer_key_id` | Pinned coordination identity reference |
| `robot_id` | Exact execution target |
| `action` | Skill name, arguments, and requested physical limits |
| `sequence` | Per-robot monotonic sequence |
| `context_hash` | SHA-256 hash of the task, zone, and world-state reference |
| `issued_at_unix_ms` | Issue time |
| `expires_at_unix_ms` | Final acceptance time |
| `constraints` | Allowed skills, zones, speed, and force |
| `risk` | `normal` or `high` |
| `approvals` | Operator assertions included in the signed token |

The implementation serializes typed Rust structures with `serde_json`. It uses ordered maps for action arguments. Do not construct token claim bytes with another serializer until cross-language canonicalization tests exist.

## Cryptography

The hybrid signature algorithm uses ML-DSA-65 from FIPS 204 and Ed25519. Verification fails if either signature fails. A domain prefix separates SPACL signatures from other protocol messages.

Identity key IDs are SHA-256 hashes of both public keys. Private identity files contain an ML-DSA seed and an Ed25519 secret. SPACL writes them with Unix mode `0600`.

The audit chain uses SHA-256. It signs each computed record hash with the source identity.

### Selected Key Establishment Primitive

SPACL selects ML-KEM-768 from FIPS 203 for future transport key establishment. The implementation uses the RustCrypto `ml-kem` crate version 0.3.2 with its `getrandom` and `zeroize` features. A unit-tested primitive generates recipient keys, encapsulates a 32-byte shared secret, and decapsulates the matching ciphertext.

This selection does not make the current HTTP interface secure. SPACL does not yet authenticate ML-KEM public keys, bind endpoint identities to a handshake transcript, derive traffic keys, encrypt messages, prevent downgrade, or rotate session keys. The upstream crate also states that it has not received an independent audit. Treat this code as an explicit dependency and parameter choice, not as a production transport claim.

## Two-Person Rule

A high-risk token requires two distinct operator IDs. The coordination node signs the operator assertions into the token. The current API does not authenticate operators and does not collect operator signatures. This control is a workflow check, not cryptographic proof of human approval.

Phase 2 must add authenticated operator accounts, separate signed approval objects, freshness checks, and role policy.

## Threats Not Controlled in v0.2.0

- Network interception, traffic analysis, or endpoint impersonation
- Coordination host compromise
- Robot host or controller compromise
- Private key extraction from software files
- Malicious or unsafe low-level robot firmware
- Sensor or world-state forgery
- Denial of service
- Clock rollback
- Byzantine state replicas
- Side-channel attacks
- Formal physical safety requirements

## Required Production Work

Add ML-KEM-based mutually authenticated transport or a reviewed hybrid Transport Layer Security (TLS) profile. Add hardware-backed keys. Authenticate human operators. Add physical safety controllers. Maintain the [repository threat model](threat-model.md). Complete an external penetration test, dependency audit, fault-injection campaign, and site hazard assessment.
