# Minimal Technology Stack

SPACL keeps the Phase 0 stack small. `Cargo.lock` fixes the exact dependency graph. CI and container builds use `--locked`.

| Area | Selection | Current use |
| --- | --- | --- |
| Language | Rust 1.88, edition 2024 | All coordinator, runtime, CLI, and cryptographic code |
| Signatures | RustCrypto `ml-dsa` 0.1.1 with ML-DSA-65 | Token, identity, and audit signatures |
| Classical signature | `ed25519-dalek` 2.2 | Required second signature in the hybrid signature bundle |
| Key establishment | RustCrypto `ml-kem` 0.3.2 with ML-KEM-768 | Selected and unit tested; not connected to transport |
| Hash | `sha2` with SHA-256 | Key IDs, context hashes, and audit chains |
| Messaging | Axum REST with JSON over Tokio | Local Phase 0 coordinator and robot APIs |
| Client | Reqwest with Rustls support | CLI calls to the local APIs |
| Schema | Serde and OpenAPI 3.1 | Typed messages and external client descriptions |
| State | Atomic JSON and JSON Lines | Coordinator state, runtime state, and audit chains |

## Locked Cryptography Parameters

- Use ML-DSA-65 for post-quantum signatures.
- Require Ed25519 as the classical signature in each hybrid signature bundle.
- Use ML-KEM-768 for post-quantum key establishment experiments.
- Use the operating system random source through each RustCrypto crate.
- Enable secret zeroization features where the crate supplies them.

## Transport Boundary

The current REST interface is plaintext HTTP. The ML-KEM primitive only proves that SPACL can generate, encapsulate, and decapsulate ML-KEM-768 keys with the locked dependency.

A later transport design must authenticate both endpoints, bind identities and protocol versions to the handshake transcript, combine classical and post-quantum key establishment when required, apply a reviewed key derivation function, encrypt all messages with an authenticated encryption algorithm, prevent downgrade, and rotate session keys. Do not describe the current interface as an ML-KEM secure channel.
